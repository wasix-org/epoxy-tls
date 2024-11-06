#[cfg(feature = "twisp")]
pub mod twisp;
pub mod utils;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use bytes::BytesMut;
use cfg_if::cfg_if;
use event_listener::Event;
use futures_util::FutureExt;
use log::{debug, trace};
use monoio::{
	io::{AsyncReadRent, AsyncWriteRentExt, Splitable},
	net::tcp::{TcpOwnedReadHalf, TcpOwnedWriteHalf},
	task::JoinHandle,
	time::interval,
};
use tokio::select;
use uuid::Uuid;
use wisp_mux::{
	ws::Payload, CloseReason, ConnectPacket, MuxStream, MuxStreamRead, MuxStreamWrite, ServerMux,
};

use crate::{
	route::WispResult,
	stream::{ClientStream, ResolvedPacket},
	CLIENTS, CONFIG,
};

async fn copy_read_fast(rx: MuxStreamRead, mut tx: TcpOwnedWriteHalf) -> anyhow::Result<()> {
	let mut res;
	while let Some(x) = rx.read().await? {
		(res, _) = tx.write_all(x).await;
		res?;
	}

	Ok(())
}

async fn copy_write_fast(tx: MuxStreamWrite, mut rx: TcpOwnedReadHalf) -> anyhow::Result<()> {
	let mut buf = Vec::with_capacity(CONFIG.stream.buffer_size);
	let mut res;
	loop {
		(res, buf) = rx.read(buf).await;
		let cnt = res?;
		if cnt == 0 {
			break Ok(());
		}
		tx.write_payload(Payload::Borrowed(&buf[0..cnt])).await?;
	}
}

async fn handle_stream(
	connect: ConnectPacket,
	muxstream: MuxStream,
	id: String,
	event: Arc<Event>,
	#[cfg(feature = "twisp")] twisp_map: twisp::TwispMap,
) {
	let requested_stream = connect.clone();

	let Ok(resolved) = ClientStream::resolve(connect).await else {
		let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
		return;
	};
	let connect = match resolved {
		ResolvedPacket::Valid(x) => x,
		ResolvedPacket::NoResolvedAddrs => {
			let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
			return;
		}
		ResolvedPacket::Blocked => {
			let _ = muxstream
				.close(CloseReason::ServerStreamBlockedAddress)
				.await;
			return;
		}
		ResolvedPacket::Invalid => {
			let _ = muxstream.close(CloseReason::ServerStreamInvalidInfo).await;
			return;
		}
	};

	let resolved_stream = connect.clone();

	let Ok(stream) = ClientStream::connect(connect).await else {
		let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
		return;
	};

	let uuid = Uuid::new_v4();

	debug!(
		"new stream created for client id {:?}: (stream uuid {:?}) {:?} {:?}",
		id, uuid, requested_stream, resolved_stream
	);

	if let Some(client) = CLIENTS.get(&id) {
		client.0.insert(uuid, (requested_stream, resolved_stream));
	}

	let forward_fut = async {
		match stream {
			ClientStream::Tcp(stream) => {
				let closer = muxstream.get_close_handle();

				let ret: anyhow::Result<()> = async {
					let (muxread, muxwrite) = muxstream.into_split();
					let (tcpread, tcpwrite) = stream.into_split();
					select! {
						x = copy_read_fast(muxread, tcpwrite) => x?,
						x = copy_write_fast(muxwrite, tcpread) => x?,
					}
					Ok(())
				}
				.await;

				match ret {
					Ok(()) => {
						let _ = closer.close(CloseReason::Voluntary).await;
					}
					Err(_) => {
						let _ = closer.close(CloseReason::Unexpected).await;
					}
				}
			}
			ClientStream::Udp(stream) => {
				let closer = muxstream.get_close_handle();

				let ret: anyhow::Result<()> = async move {
					let read = async {
						let mut data = vec![0u8; 65507];
						let mut ret;
						loop {
							(ret, data) = stream.recv(data).await;
							let size = ret?;
							if size != 0 {
								muxstream
									.write_payload(Payload::Borrowed(&data[..size]))
									.await?;
							} else {
								break Ok(());
							}
						}
					};
					let write = async {
						let mut ret;
						while let Some(data) = muxstream.read().await? {
							(ret, _) = stream.send(data).await;
							ret?;
						}
						Ok(())
					};
					select! {
						x = read => x,
						x = write => x,
					}
				}
				.await;

				match ret {
					Ok(()) => {
						let _ = closer.close(CloseReason::Voluntary).await;
					}
					Err(_) => {
						let _ = closer.close(CloseReason::Unexpected).await;
					}
				}
			}
			#[cfg(feature = "twisp")]
			ClientStream::Pty(cmd, pty) => {
				let closer = muxstream.get_close_handle();
				let id = muxstream.stream_id;
				let (mut rx, mut tx) = muxstream.into_io().into_asyncrw().into_split();

				match twisp::handle_twisp(id, &mut rx, &mut tx, twisp_map.clone(), pty, cmd).await {
					Ok(()) => {
						let _ = closer.close(CloseReason::Voluntary).await;
					}
					Err(_) => {
						let _ = closer.close(CloseReason::Unexpected).await;
					}
				}
			}
			ClientStream::Invalid => {
				let _ = muxstream.close(CloseReason::ServerStreamInvalidInfo).await;
			}
			ClientStream::Blocked => {
				let _ = muxstream
					.close(CloseReason::ServerStreamBlockedAddress)
					.await;
			}
		};
	};

	select! {
		x = forward_fut => x,
		x = event.listen() => x,
	};

	debug!("stream uuid {:?} disconnected for client id {:?}", uuid, id);

	if let Some(client) = CLIENTS.get(&id) {
		client.0.remove(&uuid);
	}
}

pub async fn handle_wisp(stream: WispResult, is_v2: bool, id: String) -> anyhow::Result<()> {
	let (read, write) = stream;
	cfg_if! {
		if #[cfg(feature = "twisp")] {
			let twisp_map = twisp::new_map();
			let (extensions, required_extensions, buffer_size) = CONFIG.wisp.to_opts().await?;

			let extensions = match extensions {
				Some(mut exts) => {
					exts.add_extension(twisp::new_ext(twisp_map.clone()));
					Some(exts)
				},
				None => {
					None
				}
			};
		} else {
			let (extensions, required_extensions, buffer_size) = CONFIG.wisp.to_opts().await?;
		}
	}

	let (mux, fut) = ServerMux::create(
		read,
		write,
		buffer_size,
		if is_v2 { extensions } else { None },
	)
	.await
	.context("failed to create server multiplexor")?
	.with_required_extensions(&required_extensions)
	.await?;
	let mux = Arc::new(mux);

	debug!(
		"new wisp client id {:?} connected with extensions {:?}, downgraded {:?}",
		id,
		mux.supported_extensions
			.iter()
			.map(|x| x.get_id())
			.collect::<Vec<_>>(),
		mux.downgraded
	);

	let mut set: Vec<JoinHandle<()>> = Vec::new();
	let event: Arc<Event> = Event::new().into();

	let mux_id = id.clone();
	set.push(monoio::spawn(fut.map(move |x| {
		debug!("wisp client id {:?} multiplexor result {:?}", mux_id, x)
	})));

	let ping_mux = mux.clone();
	let ping_event = event.clone();
	let ping_id = id.clone();
	set.push(monoio::spawn(async move {
		let mut interval = interval(Duration::from_secs(30));
		while ping_mux
			.send_ping(Payload::Bytes(BytesMut::new()))
			.await
			.is_ok()
		{
			trace!("sent ping to wisp client id {:?}", ping_id);
			select! {
				_ = interval.tick() => (),
				_ = ping_event.listen() => break,
			};
		}
	}));

	while let Some((connect, stream)) = mux.server_new_stream().await {
		set.push(monoio::spawn(handle_stream(
			connect,
			stream,
			id.clone(),
			event.clone(),
			#[cfg(feature = "twisp")]
			twisp_map.clone(),
		)));
	}

	debug!("shutting down wisp client id {:?}", id);

	let _ = mux.close().await;
	event.notify(usize::MAX);

	trace!("waiting for tasks to close for wisp client id {:?}", id);

	for task in set {
		task.await;
	}

	debug!("wisp client id {:?} disconnected", id);

	Ok(())
}
