use futures::SinkExt;

use crate::{
	locked_sink::LockedWebSocketWrite,
	packet::{CloseReason, ConnectPacket, MaybeInfoPacket, Packet, StreamType},
	stream::MuxStream,
	ws::{Payload, TransportRead, TransportReadExt, TransportWrite},
	Role, WispError,
};

use super::{
	get_supported_extensions, handle_handshake,
	inner::{FlowControl, MultiplexorActor, StreamMap},
	send_info_packet, Multiplexor, MultiplexorImpl, MuxResult, WispHandshakeResult,
	WispHandshakeResultKind, WispV2Handshake,
};

pub(crate) struct ServerActor<W: TransportWrite> {
	stream_tx: flume::Sender<(ConnectPacket, MuxStream<W>)>,
}

impl<W: TransportWrite> MultiplexorActor<W> for ServerActor<W> {
	fn handle_connect_packet(
		&mut self,
		stream: MuxStream<W>,
		pkt: ConnectPacket,
	) -> Result<(), WispError> {
		self.stream_tx
			.send((pkt, stream))
			.map_err(|_| WispError::MuxMessageFailedToSend)
	}

	fn handle_data_packet(
		&mut self,
		id: u32,
		pkt: Payload,
		streams: &mut StreamMap,
	) -> Result<(), WispError> {
		if let Some(stream) = streams.get(&id) {
			if stream.stream.try_send(pkt).is_ok() {
				stream.info.flow_dec();
			}
		}
		Ok(())
	}

	fn handle_continue_packet(
		&mut self,
		_: u32,
		_: crate::packet::ContinuePacket,
		_: &mut StreamMap,
	) -> Result<(), WispError> {
		Err(WispError::InvalidPacketType(0x03))
	}

	fn get_flow_control(ty: StreamType, flow_stream_types: &[u8]) -> FlowControl {
		if flow_stream_types.contains(&ty.into()) {
			FlowControl::EnabledSendMessages
		} else {
			FlowControl::Disabled
		}
	}
}

pub struct ServerImpl<W: TransportWrite> {
	buffer_size: u32,
	stream_rx: flume::Receiver<(ConnectPacket, MuxStream<W>)>,
}

impl<W: TransportWrite> MultiplexorImpl<W> for ServerImpl<W> {
	type Actor = ServerActor<W>;

	async fn handshake<R: TransportRead>(
		&mut self,
		rx: &mut R,
		tx: &mut LockedWebSocketWrite<W>,
		v2: Option<WispV2Handshake>,
	) -> Result<WispHandshakeResult, WispError> {
		if let Some(WispV2Handshake {
			mut builders,
			closure,
		}) = v2
		{
			send_info_packet(tx, &mut builders, Role::Server).await?;

			(closure)(&mut builders).await?;

			let packet =
				MaybeInfoPacket::decode(rx.next_erroring().await?, &mut builders, Role::Server)?;

			match packet {
				MaybeInfoPacket::Info(info) => {
					let mut supported_extensions =
						get_supported_extensions(info.extensions, &mut builders);

					handle_handshake(rx, tx, &mut supported_extensions).await?;

					tx.lock().await;
					tx.get()
						.send(Packet::new_continue(0, self.buffer_size).encode())
						.await?;
					tx.unlock();

					// v2 client
					Ok(WispHandshakeResult {
						kind: WispHandshakeResultKind::V2 {
							extensions: supported_extensions,
						},
						downgraded: false,
						buffer_size: self.buffer_size,
					})
				}
				MaybeInfoPacket::Packet(packet) => {
					// downgrade to v1
					Ok(WispHandshakeResult {
						kind: WispHandshakeResultKind::V1 {
							packet: Some(packet),
						},
						downgraded: true,
						buffer_size: self.buffer_size,
					})
				}
			}
		} else {
			// user asked for v1 server
			tx.lock().await;
			tx.get()
				.send(Packet::new_continue(0, self.buffer_size).encode())
				.await?;
			tx.unlock();

			Ok(WispHandshakeResult {
				kind: WispHandshakeResultKind::V1 { packet: None },
				downgraded: false,
				buffer_size: self.buffer_size,
			})
		}
	}

	async fn handle_error(
		&mut self,
		err: WispError,
		tx: &mut LockedWebSocketWrite<W>,
	) -> Result<WispError, WispError> {
		match err {
			WispError::PasswordExtensionCredsInvalid => {
				tx.lock().await;
				tx.get()
					.send(Packet::new_close(0, CloseReason::ExtensionsPasswordAuthFailed).encode())
					.await?;
				tx.get().close().await?;
				tx.unlock();
				Ok(err)
			}
			WispError::CertAuthExtensionSigInvalid => {
				tx.lock().await;
				tx.get()
					.send(Packet::new_close(0, CloseReason::ExtensionsCertAuthFailed).encode())
					.await?;
				tx.get().close().await?;
				tx.unlock();
				Ok(err)
			}
			x => Ok(x),
		}
	}
}

impl<W: TransportWrite> Multiplexor<ServerImpl<W>, W> {
	/// Create a new server-side multiplexor.
	///
	/// If `wisp_v2` is None a Wisp v1 connection is created, otherwise a Wisp v2 connection is created.
	/// **It is not guaranteed that all extensions you specify are available.** You must manually check
	/// if the extensions you need are available after the multiplexor has been created.
	#[expect(clippy::new_ret_no_self)]
	pub async fn new<R: TransportRead>(
		rx: R,
		tx: W,
		buffer_size: u32,
		wisp_v2: Option<WispV2Handshake>,
	) -> Result<MuxResult<ServerImpl<W>, W>, WispError> {
		let (stream_tx, stream_rx) = flume::unbounded();

		let mux = ServerImpl {
			buffer_size,
			stream_rx,
		};
		let actor = ServerActor { stream_tx };

		Self::create(rx, tx, wisp_v2, mux, actor).await
	}

	/// Wait for a stream to be created.
	pub async fn wait_for_stream(&self) -> Option<(ConnectPacket, MuxStream<W>)> {
		self.mux.stream_rx.recv_async().await.ok()
	}
}
