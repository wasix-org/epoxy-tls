use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use js_sys::{Function, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wisp_mux::{
	Role, WispError, extensions::{AnyProtocolExtension, ProtocolExtension, ProtocolExtensionBuilder}, packet::CloseReason, ws::{Payload, TransportRead, TransportWrite}
};

use crate::{
	EpoxyError,
	js_types::{
		JsProtocolExtensionBuilderFromBytes, JsProtocolExtensionBuilderToExtension,
		JsProtocolExtensionEncode, JsProtocolExtensionHandshake, JsProtocolExtensionPacket,
	},
	refstruct,
};

use super::extension::RefScope;

#[wasm_bindgen(inline_js = "export let to_protocol_extension = x => x;")]
extern "C" {
	fn to_protocol_extension(val: JsValue) -> JsProtocolExtension;
}

refstruct!(dyn TransportRead, JsTransportRead);
#[wasm_bindgen]
impl JsTransportRead {
	pub async fn read(&mut self) -> Result<Option<Uint8Array>, EpoxyError> {
		let x = self.inner()?.next().await.transpose()?;
		Ok(x.map(|x| Uint8Array::from(x.as_ref())))
	}
}

refstruct!(dyn TransportWrite, JsTransportWrite);
#[wasm_bindgen]
impl JsTransportWrite {
	pub async fn write(&mut self, bytes: Uint8Array) -> Result<(), EpoxyError> {
		self.inner()?.send(Payload::from(bytes.to_vec())).await?;

		Ok(())
	}
}

pub fn js_role(role: Role) -> JsValue {
	match role {
		Role::Server => "server".into(),
		Role::Client => "client".into(),
	}
}

#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsProtocolExtensionBuilder {
	id: u8,
	from_bytes: Function,
	to_extension: Function,

	#[wasm_bindgen(skip)]
	pub js_host: JsValue,
}
#[wasm_bindgen]
impl JsProtocolExtensionBuilder {
	#[wasm_bindgen(constructor)]
	pub fn new(
		id: u8,
		from_bytes: JsProtocolExtensionBuilderFromBytes,
		to_extension: JsProtocolExtensionBuilderToExtension,
		host: JsValue,
	) -> Self {
		Self {
			id,
			from_bytes: from_bytes.unchecked_into(),
			to_extension: to_extension.unchecked_into(),
			js_host: host,
		}
	}
}
#[async_trait]
impl ProtocolExtensionBuilder for JsProtocolExtensionBuilder {
	fn get_id(&self) -> u8 {
		self.id
	}

	fn build_from_bytes(
		&mut self,
		bytes: Bytes,
		role: Role,
	) -> Result<AnyProtocolExtension, WispError> {
		self.from_bytes
			.call2(
				&JsValue::NULL,
				&Uint8Array::new_from_slice(&bytes).into(),
				&js_role(role),
			)
			.map(|x| AnyProtocolExtension::new(to_protocol_extension(x)))
			.map_err(|x| WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x))))
	}

	fn build_to_extension(&mut self, role: Role) -> Result<AnyProtocolExtension, WispError> {
		self.to_extension
			.call1(&JsValue::NULL, &js_role(role))
			.map(|x| AnyProtocolExtension::new(to_protocol_extension(x)))
			.map_err(|x| WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x))))
	}
}

#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsProtocolExtension {
	id: u8,
	supported_packets: Vec<u8>,
	congestion_stream_types: Vec<u8>,
	encode: Function,
	handshake: Function,
	packet: Function,

	#[wasm_bindgen(skip)]
	pub js_host: JsValue,
}
#[wasm_bindgen]
impl JsProtocolExtension {
	#[wasm_bindgen(constructor)]
	pub fn new(
		id: u8,
		supported_packets: Vec<u8>,
		congestion_stream_types: Vec<u8>,
		encode: JsProtocolExtensionEncode,
		handshake: JsProtocolExtensionHandshake,
		packet: JsProtocolExtensionPacket,
		host: JsValue,
	) -> Self {
		Self {
			id,
			supported_packets,
			congestion_stream_types,
			encode: encode.unchecked_into(),
			handshake: handshake.unchecked_into(),
			packet: packet.unchecked_into(),
			js_host: host,
		}
	}
}
#[async_trait]
impl ProtocolExtension for JsProtocolExtension {
	fn get_id(&self) -> u8 {
		self.id
	}

	fn get_supported_packets(&self) -> &[u8] {
		&self.supported_packets
	}

	fn get_congestion_stream_types(&self) -> &[u8] {
		&self.congestion_stream_types
	}

	fn encode(&self) -> Bytes {
		let ret = self
			.encode
			.call0(&JsValue::NULL)
			.unwrap()
			.unchecked_into::<Uint8Array>();

		ret.to_vec().into()
	}

	// TODO expose CloseCode, WispError path
	async fn handle_handshake(
		&mut self,
		read: &mut dyn TransportRead,
		write: &mut dyn TransportWrite,
	) -> Result<Option<(CloseReason, WispError)>, WispError> {
		let scope = RefScope::new();
		let read: JsTransportRead = (read, scope.token()).into();
		let write: JsTransportWrite = (write, scope.token()).into();

		self.handshake
			.call2(&JsValue::NULL, &read.into(), &write.into())
			.map(|_| None)
			.map_err(|x| WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x))))
	}

	async fn handle_packet(
		&mut self,
		packet_type: u8,
		packet: Bytes,
		read: &mut dyn TransportRead,
		write: &mut dyn TransportWrite,
	) -> Result<(), WispError> {
		let scope = RefScope::new();
		let read: JsTransportRead = (read, scope.token()).into();
		let write: JsTransportWrite = (write, scope.token()).into();

		self.packet
			.call4(
				&JsValue::NULL,
				&packet_type.into(),
				&Uint8Array::new_from_slice(&packet).into(),
				&read.into(),
				&write.into(),
			)
			.map(|_| ())
			.map_err(|x| WispError::ExtensionImplError(Box::new(EpoxyError::js_error(x))))
	}

	fn box_clone(&self) -> Box<dyn ProtocolExtension + Sync + Send> {
		Box::new(self.clone())
	}
}
