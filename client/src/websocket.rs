use std::sync::Arc;

use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, Payload, Role, WebSocket};
use futures::lock::Mutex;
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen(typescript_custom_section)]
const WS_TYPES_TS: &'static str = r#"
export type WsReadEvent =
	| { type: "text"; data: string }
	| { type: "binary"; data: Uint8Array }
	| { type: "close"; code?: number; reason?: string };

export type WsUpgrade = [
	ClientResponse,
	ReadableStream<WsReadEvent>,
	WritableStream<WsWriteEvent>,
];
"#;

enum WsWriteEv {
	Text(String),
	Binary(Vec<u8>),
	Close {
		code: Option<u16>,
		reason: Option<String>,
	},
}

#[wasm_bindgen]
pub struct WsWriteEvent(WsWriteEv);

#[wasm_bindgen(inline_js = "export let to_ws_write_event = x => x;")]
extern "C" {
	#[wasm_bindgen(catch)]
	fn to_ws_write_event(val: JsValue) -> Result<WsWriteEvent, JsValue>;
}

#[wasm_bindgen]
impl WsWriteEvent {
	pub fn text(data: String) -> Self {
		Self(WsWriteEv::Text(data))
	}

	pub fn binary(data: Uint8Array) -> Self {
		Self(WsWriteEv::Binary(data.to_vec()))
	}

	pub fn close(code: Option<u16>, reason: Option<String>) -> Self {
		Self(WsWriteEv::Close { code, reason })
	}
}

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(typescript_type = "WsReadEvent")]
	pub type WsReadEvent;

	#[wasm_bindgen(typescript_type = "WsUpgrade")]
	pub type WsUpgrade;
}

fn js_ws_error(msg: impl Into<String>) -> JsValue {
	JsValue::from_str(&msg.into())
}

fn ws_read_event(frame: Frame<'_>) -> Result<Option<JsValue>, JsValue> {
	let ev = Object::new();

	match frame.opcode {
		OpCode::Text => {
			let data = std::str::from_utf8(&frame.payload)
				.map_err(|x| js_ws_error(format!("invalid websocket text payload: {x}")))?;
			Reflect::set(&ev, &"type".into(), &"text".into())?;
			Reflect::set(&ev, &"data".into(), &data.into())?;
		}
		OpCode::Binary => {
			Reflect::set(&ev, &"type".into(), &"binary".into())?;
			Reflect::set(
				&ev,
				&"data".into(),
				&Uint8Array::new_from_slice(&frame.payload).into(),
			)?;
		}
		OpCode::Close => {
			Reflect::set(&ev, &"type".into(), &"close".into())?;
			if frame.payload.len() >= 2 {
				let code = u16::from_be_bytes([frame.payload[0], frame.payload[1]]) as f64;
				Reflect::set(&ev, &"code".into(), &JsValue::from_f64(code))?;

				if frame.payload.len() > 2 {
					let reason = String::from_utf8_lossy(&frame.payload[2..]);
					Reflect::set(&ev, &"reason".into(), &reason.as_ref().into())?;
				}
			}
		}
		OpCode::Ping | OpCode::Pong => return Ok(None),
		OpCode::Continuation => {
			return Err(js_ws_error("unexpected websocket continuation frame"));
		}
	}

	Ok(Some(ev.into()))
}

fn ws_write_event(value: JsValue) -> Result<Frame<'static>, JsValue> {
	let event =
		to_ws_write_event(value).map_err(|_| js_ws_error("expected a WsWriteEvent instance"))?;

	match event.0 {
		WsWriteEv::Text(data) => Ok(Frame::text(Payload::Owned(data.into_bytes()))),
		WsWriteEv::Binary(data) => Ok(Frame::binary(Payload::Owned(data))),
		WsWriteEv::Close { code, reason } => {
			if code.is_none() && reason.is_none() {
				return Ok(Frame::close_raw(Payload::Owned(vec![])));
			}

			let code = code.unwrap_or(1000);
			let reason = reason.unwrap_or_default();
			Ok(Frame::close(code, reason.as_bytes()))
		}
	}
}

pub(crate) fn websocket_streams(
	upgraded: Upgraded,
) -> (web_sys::ReadableStream, web_sys::WritableStream) {
	let websocket = WebSocket::after_handshake(TokioIo::new(upgraded), Role::Client);
	let (mut ws_read, ws_write) = websocket.split(tokio::io::split);
	ws_read.set_auto_close(true);
	ws_read.set_auto_pong(true);

	let ws_write = Arc::new(Mutex::new(ws_write));

	let ws_read = FragmentCollectorRead::new(ws_read);

	let read = wasm_streams::ReadableStream::from_stream(futures::stream::unfold(
		(ws_read, ws_write.clone(), false),
		|(mut ws_read, ws_write, closed)| async move {
			if closed {
				return None;
			}

			loop {
				let write_for_send = ws_write.clone();
				let mut send = move |frame| {
					let write_for_send = write_for_send.clone();
					async move {
						let mut ws_write = write_for_send.lock().await;
						ws_write
							.write_frame(frame)
							.await
							.map_err(std::io::Error::other)?;
						ws_write.flush().await.map_err(std::io::Error::other)
					}
				};

				match ws_read.read_frame(&mut send).await {
					Ok(frame) => {
						let done = frame.opcode == OpCode::Close;
						match ws_read_event(frame) {
							Ok(Some(event)) => return Some((Ok(event), (ws_read, ws_write, done))),
							Ok(None) => {}
							Err(err) => return Some((Err(err), (ws_read, ws_write, true))),
						}
					}
					Err(err) => {
						return Some((
							Err(js_ws_error(format!("websocket read error: {err}"))),
							(ws_read, ws_write, true),
						));
					}
				}
			}
		},
	))
	.into_raw();

	let write = wasm_streams::WritableStream::from_sink(futures::sink::unfold(
		ws_write,
		|ws_write, event: JsValue| async move {
			let frame = ws_write_event(event)?;
			let mut guard = ws_write.lock().await;
			guard
				.write_frame(frame)
				.await
				.map_err(|x| js_ws_error(format!("websocket write error: {x}")))?;
			guard
				.flush()
				.await
				.map_err(|x| js_ws_error(format!("websocket write flush error: {x}")))?;
			drop(guard);

			Ok(ws_write)
		},
	))
	.into_raw();

	(read, write)
}
