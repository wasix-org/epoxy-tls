//! Opt-in Wasmer TCP keepalive extension. See `wasmer/tcp-keepalive.md`.
use std::{
	collections::HashMap,
	os::fd::{AsFd, AsRawFd, OwnedFd},
	sync::{Arc, Mutex},
};

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::watch;
use wisp_mux::{
	extensions::{AnyProtocolExtension, ProtocolExtension, ProtocolExtensionBuilder},
	packet::StreamType,
	stream::MuxStream,
	ws::TransportWrite,
	Role, WispError,
};

use crate::stream::ClientStream;

const EXTENSION_ID: u8 = 0xF2;
const REQUEST_PACKET: u8 = 0xF2;
const REPLY_PACKET: u8 = 0xF3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Status {
	Invalid = 1,
	Unsupported = 2,
	BadStream = 3,
	Io = 4,
}

#[derive(Debug, Clone)]
enum SocketState {
	Connecting,
	Connected(Arc<OwnedFd>),
	Failed(Status),
}

#[derive(Debug, Clone, Default)]
pub struct SocketRegistry(Arc<Mutex<HashMap<u32, watch::Sender<SocketState>>>>);

impl SocketRegistry {
	fn open(&self, id: u32, ty: StreamType) {
		let state = if ty == StreamType::Tcp {
			SocketState::Connecting
		} else {
			SocketState::Failed(Status::Unsupported)
		};
		self.0.lock().unwrap().insert(id, watch::channel(state).0);
	}

	fn close(&self, id: u32) {
		if let Some(sender) = self.0.lock().unwrap().remove(&id) {
			sender.send_replace(SocketState::Failed(Status::BadStream));
		}
	}

	pub fn register_stream<W: TransportWrite>(&self, stream: &MuxStream<W>) -> SocketRegistration {
		let sockets = self.0.lock().unwrap();
		let sender = if stream.get_close_reason().is_none() {
			sockets.get(&stream.get_stream_id()).cloned()
		} else {
			None
		};
		SocketRegistration {
			registry: self.clone(),
			stream_id: stream.get_stream_id(),
			sender,
		}
	}

	#[cfg(test)]
	fn attach(&self, id: u32, stream: &ClientStream) -> SocketRegistration {
		let registration = SocketRegistration {
			registry: self.clone(),
			stream_id: id,
			sender: self.0.lock().unwrap().get(&id).cloned(),
		};
		registration.attach(stream);
		registration
	}

	async fn socket(&self, id: u32) -> Result<Arc<OwnedFd>, Status> {
		let mut receiver = self
			.0
			.lock()
			.unwrap()
			.get(&id)
			.ok_or(Status::BadStream)?
			.subscribe();
		loop {
			match receiver.borrow_and_update().clone() {
				SocketState::Connected(fd) => return Ok(fd),
				SocketState::Failed(status) => return Err(status),
				SocketState::Connecting => {}
			}
			receiver.changed().await.map_err(|_| Status::BadStream)?;
		}
	}

	async fn request(&self, id: u32, operation: u8, option: u8, value: u32) -> Result<u32, Status> {
		if operation > 1
			|| option > 3
			|| (operation == 0
				&& ((option == 0 && value > 1)
					|| (option != 0 && (value == 0 || value > i32::MAX as u32))))
		{
			return Err(Status::Invalid);
		}
		let fd = self.socket(id).await?;
		if operation == 0 {
			socket_option(&fd, option, Some(value))?;
		}
		// ACK values and GET values both come from the socket, never a cache.
		socket_option(&fd, option, None)
	}
}

/// Retire the duplicated descriptor when forwarding ends or is cancelled.
pub struct SocketRegistration {
	registry: SocketRegistry,
	stream_id: u32,
	sender: Option<watch::Sender<SocketState>>,
}
impl SocketRegistration {
	pub fn attach(&self, stream: &ClientStream) {
		let Some(sender) = &self.sender else {
			return;
		};
		sender.send_if_modified(|state| {
			if !matches!(state, SocketState::Connecting) {
				return false;
			}
			*state = match stream {
				ClientStream::Tcp(stream) => match stream.as_fd().try_clone_to_owned() {
					Ok(fd) => SocketState::Connected(Arc::new(fd)),
					Err(error) => {
						log::debug!("keepalive descriptor duplication failed: {error}");
						SocketState::Failed(Status::Io)
					}
				},
				_ => SocketState::Failed(Status::Unsupported),
			};
			true
		});
	}
}
impl Drop for SocketRegistration {
	fn drop(&mut self) {
		let Some(sender) = &self.sender else {
			return;
		};
		let mut sockets = self.registry.0.lock().unwrap();
		// A client can reuse a closed stream ID. An older forwarding task must
		// never retire or attach its socket to that newer stream.
		if sockets
			.get(&self.stream_id)
			.is_some_and(|current| current.same_channel(sender))
		{
			sockets.remove(&self.stream_id);
		}
		sender.send_replace(SocketState::Failed(Status::BadStream));
	}
}

#[derive(Debug, Clone)]
struct KeepaliveExtension(SocketRegistry);

#[async_trait]
impl ProtocolExtension for KeepaliveExtension {
	fn get_id(&self) -> u8 {
		EXTENSION_ID
	}
	fn get_supported_packets(&self) -> &'static [u8] {
		&[REQUEST_PACKET]
	}
	fn encode(&self) -> Bytes {
		Bytes::from_static(&[1])
	}
	fn on_stream_open(&mut self, id: u32, ty: StreamType) {
		self.0.open(id, ty);
	}
	fn on_stream_close(&mut self, id: u32) {
		self.0.close(id);
	}
	fn box_clone(&self) -> Box<dyn ProtocolExtension + Sync + Send> {
		Box::new(self.clone())
	}

	async fn handle_stream_packet(
		&mut self,
		_: u8,
		stream_id: u32,
		mut packet: Bytes,
	) -> Result<Option<Bytes>, WispError> {
		if packet.len() < 4 {
			return Err(WispError::PacketTooSmall);
		}
		let request_id = packet.get_u32_le();
		let result = if packet.len() == 6 {
			let operation = packet.get_u8();
			let option = packet.get_u8();
			let value = packet.get_u32_le();
			self.0.request(stream_id, operation, option, value).await
		} else {
			Err(Status::Invalid)
		};
		let mut reply = BytesMut::with_capacity(14);
		reply.put_u8(REPLY_PACKET);
		reply.put_u32_le(stream_id);
		reply.put_u32_le(request_id);
		match result {
			Ok(value) => {
				reply.put_u8(0);
				reply.put_u32_le(value);
			}
			Err(status) => {
				reply.put_u8(status as u8);
				reply.put_u32_le(0);
			}
		}
		Ok(Some(reply.freeze()))
	}
}

pub struct KeepaliveBuilder(pub SocketRegistry);
impl ProtocolExtensionBuilder for KeepaliveBuilder {
	fn get_id(&self) -> u8 {
		EXTENSION_ID
	}
	fn build_from_bytes(
		&mut self,
		bytes: Bytes,
		role: Role,
	) -> Result<AnyProtocolExtension, WispError> {
		if bytes.as_ref() != [1] {
			return Err(WispError::ExtensionImplNotSupported);
		}
		self.build_to_extension(role)
	}
	fn build_to_extension(&mut self, role: Role) -> Result<AnyProtocolExtension, WispError> {
		if !matches!(role, Role::Server) {
			return Err(WispError::ExtensionImplNotSupported);
		}
		Ok(KeepaliveExtension(self.0.clone()).into())
	}
}

#[cfg(not(target_os = "wasi"))]
fn socket_option(fd: &OwnedFd, option: u8, set: Option<u32>) -> Result<u32, Status> {
	let (level, name) = match option {
		0 => (libc::SOL_SOCKET, libc::SO_KEEPALIVE),
		#[cfg(any(target_os = "macos", target_os = "ios"))]
		1 => (libc::IPPROTO_TCP, libc::TCP_KEEPALIVE),
		#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
		1 => (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE),
		#[cfg(any(
			target_os = "macos",
			target_os = "ios",
			target_os = "linux",
			target_os = "android",
			target_os = "freebsd"
		))]
		2 => (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL),
		#[cfg(any(
			target_os = "macos",
			target_os = "ios",
			target_os = "linux",
			target_os = "android",
			target_os = "freebsd"
		))]
		3 => (libc::IPPROTO_TCP, libc::TCP_KEEPCNT),
		_ => return Err(Status::Unsupported),
	};
	let mut value: libc::c_int = set.unwrap_or(0).try_into().map_err(|_| Status::Invalid)?;
	let mut size = libc::socklen_t::try_from(size_of::<libc::c_int>()).map_err(|_| Status::Io)?;
	// SAFETY: fd owns a live socket; both syscalls receive an initialized c_int
	// and its exact size. Setting a timing option does not toggle SO_KEEPALIVE.
	let result = unsafe {
		if set.is_some() {
			libc::setsockopt(fd.as_raw_fd(), level, name, (&raw const value).cast(), size)
		} else {
			libc::getsockopt(
				fd.as_raw_fd(),
				level,
				name,
				(&raw mut value).cast(),
				&raw mut size,
			)
		}
	};
	if result != 0 {
		let error = std::io::Error::last_os_error();
		log::debug!("keepalive socket option {option} failed: {error}");
		return Err(match error.raw_os_error() {
			Some(libc::EINVAL) => Status::Invalid,
			Some(code)
				if code == libc::ENOPROTOOPT
					|| code == libc::EOPNOTSUPP
					|| code == libc::ENOSYS =>
			{
				Status::Unsupported
			}
			_ => Status::Io,
		});
	}
	if option == 0 {
		Ok(u32::from(value != 0))
	} else {
		u32::try_from(value).map_err(|_| Status::Io)
	}
}

// Call the WASIX option ABI directly: old WASIX libc builds do not map the
// TCP_KEEP* POSIX options. This requires a runtime supporting options 27–29.
#[cfg(target_os = "wasi")]
fn socket_option(fd: &OwnedFd, option: u8, set: Option<u32>) -> Result<u32, Status> {
	#[link(wasm_import_module = "wasix_32v1")]
	unsafe extern "C" {
		fn sock_set_opt_flag(fd: u32, option: u8, value: u8) -> u16;
		fn sock_get_opt_flag(fd: u32, option: u8, value: *mut u8) -> u16;
		fn sock_set_opt_size(fd: u32, option: u8, value: u64) -> u16;
		fn sock_get_opt_size(fd: u32, option: u8, value: *mut u64) -> u16;
	}
	let fd = fd.as_raw_fd() as u32;
	let mut flag = 0_u8;
	let mut size = 0_u64;
	// SAFETY: the descriptor is live and output pointers have the ABI's size.
	let errno = unsafe {
		match (option, set) {
			(0, Some(value)) => sock_set_opt_flag(fd, 12, value as u8),
			(0, None) => sock_get_opt_flag(fd, 12, &raw mut flag),
			(1..=3, Some(value)) => sock_set_opt_size(fd, 26 + option, value.into()),
			(1..=3, None) => sock_get_opt_size(fd, 26 + option, &raw mut size),
			_ => return Err(Status::Unsupported),
		}
	};
	match errno {
		0 => {
			if option == 0 {
				Ok(flag.into())
			} else {
				size.try_into().map_err(|_| Status::Io)
			}
		}
		28 => Err(Status::Invalid),
		50 | 52 | 58 => Err(Status::Unsupported),
		_ => {
			log::debug!("keepalive WASIX option {option} failed: errno {errno}");
			Err(Status::Io)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures_util::{future::ready, sink, stream, StreamExt};
	use std::{pin::Pin, time::Duration};
	use tokio::{
		net::{TcpListener, TcpStream},
		sync::mpsc,
		time::timeout,
	};
	use wisp_mux::{packet::CloseReason, ws::TransportWrite, ServerMux, WispV2Handshake};

	type TestWrite = Pin<Box<dyn TransportWrite>>;
	type TestMux = ServerMux<TestWrite>;

	async fn socket_pair() -> (ClientStream, TcpStream) {
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let (outgoing, incoming) = tokio::join!(
			TcpStream::connect(listener.local_addr().unwrap()),
			listener.accept()
		);
		(ClientStream::Tcp(outgoing.unwrap()), incoming.unwrap().0)
	}

	#[tokio::test]
	async fn keepalive_kernel_readback_and_enable_state() {
		let (stream, _peer) = socket_pair().await;
		let registry = SocketRegistry::default();
		registry.open(7, StreamType::Tcp);
		let registration = registry.attach(7, &stream);
		assert_eq!(registry.request(7, 0, 0, 0).await, Ok(0));
		for (option, value) in [(1, 23), (2, 7), (3, 4)] {
			assert_eq!(registry.request(7, 0, option, value).await, Ok(value));
			assert_eq!(registry.request(7, 1, option, 0).await, Ok(value));
			assert_eq!(registry.request(7, 1, 0, 0).await, Ok(0));
		}
		assert_eq!(registry.request(7, 0, 0, 1).await, Ok(1));
		assert_eq!(registry.request(7, 0, 1, 31).await, Ok(31));
		assert_eq!(registry.request(7, 1, 0, 0).await, Ok(1));
		assert_eq!(registry.request(7, 1, 2, 0).await, Ok(7));
		assert_eq!(registry.request(7, 1, 3, 0).await, Ok(4));
		for (operation, option, value) in
			[(0, 0, 2), (0, 1, 0), (0, 2, u32::MAX), (2, 1, 0), (1, 4, 0)]
		{
			assert_eq!(
				registry.request(7, operation, option, value).await,
				Err(Status::Invalid)
			);
		}
		// A real getsockopt failure must remain an error, never cached success.
		let non_socket: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
		assert_eq!(socket_option(&non_socket, 0, None), Err(Status::Io));
		assert_eq!(registry.request(7, 1, 3, 0).await, Ok(4));
		drop(registration);
		assert_eq!(registry.request(7, 1, 0, 0).await, Err(Status::BadStream));
		registry.open(8, StreamType::Udp);
		assert_eq!(registry.request(8, 1, 0, 0).await, Err(Status::Unsupported));
	}

	fn info(version: Option<u8>) -> Bytes {
		let mut bytes = BytesMut::from(&[5, 0, 0, 0, 0, 2, 0][..]);
		if let Some(version) = version {
			bytes.put_u8(EXTENSION_ID);
			bytes.put_u32_le(1);
			bytes.put_u8(version);
		}
		bytes.freeze()
	}

	fn request(id: u32, sequence: u32, operation: u8, option: u8, value: u32) -> Bytes {
		let mut bytes = BytesMut::new();
		bytes.put_u8(REQUEST_PACKET);
		bytes.put_u32_le(id);
		bytes.put_u32_le(sequence);
		bytes.put_u8(operation);
		bytes.put_u8(option);
		bytes.put_u32_le(value);
		bytes.freeze()
	}

	fn connect(id: u32) -> Bytes {
		let mut bytes = BytesMut::new();
		bytes.put_u8(1);
		bytes.put_u32_le(id);
		bytes.put_u8(1);
		bytes.put_u16_le(80);
		bytes.extend_from_slice(b"127.0.0.1");
		bytes.freeze()
	}

	async fn next_reply(rx: &mut mpsc::UnboundedReceiver<Bytes>) -> Bytes {
		timeout(Duration::from_secs(2), async {
			loop {
				let frame = rx.recv().await.unwrap();
				if frame[0] == REPLY_PACKET {
					return frame;
				}
			}
		})
		.await
		.expect("keepalive response must not deadlock")
	}

	async fn mux(
		negotiated: bool,
	) -> (
		TestMux,
		tokio::task::JoinHandle<Result<(), WispError>>,
		SocketRegistry,
		mpsc::UnboundedSender<Bytes>,
		mpsc::UnboundedReceiver<Bytes>,
	) {
		let (input, receiver) = mpsc::unbounded_channel();
		let (sender, mut output) = mpsc::unbounded_channel();
		let read = Box::pin(stream::unfold(receiver, |mut receiver| async move {
			receiver.recv().await.map(|bytes| (Ok(bytes), receiver))
		}));
		let write: TestWrite = Box::pin(sink::unfold(
			sender,
			|sender: mpsc::UnboundedSender<Bytes>, bytes| {
				ready(
					sender
						.send(bytes)
						.map(|()| sender)
						.map_err(|_| WispError::WsImplSocketClosed),
				)
			},
		));
		let registry = SocketRegistry::default();
		input.send(info(negotiated.then_some(1))).unwrap();
		let (mux, future) = ServerMux::new(
			read,
			write,
			128,
			Some(WispV2Handshake::new(vec![KeepaliveBuilder(
				registry.clone(),
			)
			.into()])),
		)
		.await
		.unwrap()
		.with_no_required_extensions();
		assert_eq!(output.recv().await.unwrap(), info(Some(1)));
		assert_eq!(output.recv().await.unwrap()[0], 3);
		assert_eq!(mux.get_extension_ids().contains(&EXTENSION_ID), negotiated);
		(mux, tokio::spawn(future), registry, input, output)
	}

	#[tokio::test]
	async fn keepalive_negotiated_requests_wait_for_connect_and_continue_reading() {
		let (mux, task, registry, input, mut output) = mux(true).await;
		input.send(connect(9)).unwrap();
		input.send(request(9, 42, 0, 1, 29)).unwrap();
		let (_connect, mut stream) = mux.wait_for_stream().await.unwrap();
		assert!(timeout(Duration::from_millis(20), output.recv())
			.await
			.is_err());
		let (socket, _peer) = socket_pair().await;
		let registration = registry.register_stream(&stream);
		registration.attach(&socket);
		let reply = next_reply(&mut output).await;
		assert_eq!(
			reply.as_ref(),
			&[REPLY_PACKET, 9, 0, 0, 0, 42, 0, 0, 0, 0, 29, 0, 0, 0]
		);
		input.send(request(9, 43, 1, 1, 0)).unwrap();
		assert_eq!(next_reply(&mut output).await[10..], 29_u32.to_le_bytes());
		input.send(request(9, 44, 1, 0, 0)).unwrap();
		assert_eq!(next_reply(&mut output).await[10..], 0_u32.to_le_bytes());
		// Ordinary traffic must still be delivered after control packets.
		input
			.send(Bytes::from_static(&[2, 9, 0, 0, 0, 72, 73]))
			.unwrap();
		assert_eq!(
			timeout(Duration::from_secs(2), stream.next())
				.await
				.unwrap()
				.unwrap()
				.unwrap(),
			Bytes::from_static(b"HI")
		);
		input.send(request(9, 45, 0, 0, 2)).unwrap();
		assert_eq!(next_reply(&mut output).await[9], Status::Invalid as u8);
		timeout(Duration::from_secs(2), stream.close(CloseReason::Voluntary))
			.await
			.unwrap()
			.unwrap();
		input.send(request(9, 46, 1, 0, 0)).unwrap();
		assert_eq!(next_reply(&mut output).await[9], Status::BadStream as u8);
		// Reusing a closed stream ID cannot let an old forwarding task attach
		// its socket to or retire the new stream.
		input.send(connect(9)).unwrap();
		let (_, replacement) = mux.wait_for_stream().await.unwrap();
		let (replacement_socket, _replacement_peer) = socket_pair().await;
		let replacement_registration = registry.register_stream(&replacement);
		replacement_registration.attach(&replacement_socket);
		registration.attach(&socket);
		drop(registration);
		input.send(request(9, 47, 0, 1, 37)).unwrap();
		assert_eq!(next_reply(&mut output).await[10..], 37_u32.to_le_bytes());
		drop(replacement_registration);
		drop(input);
		timeout(Duration::from_secs(2), task)
			.await
			.unwrap()
			.unwrap()
			.unwrap();
	}

	#[tokio::test]
	async fn keepalive_connect_failure_wakes_pending_request_without_write_lock_deadlock() {
		let (mux, task, _registry, input, mut output) = mux(true).await;
		input.send(connect(11)).unwrap();
		input.send(request(11, 1, 0, 0, 1)).unwrap();
		let (_, stream) = mux.wait_for_stream().await.unwrap();
		assert!(timeout(Duration::from_millis(20), output.recv())
			.await
			.is_err());
		timeout(
			Duration::from_secs(2),
			stream.close(CloseReason::ServerStreamUnreachable),
		)
		.await
		.unwrap()
		.unwrap();
		assert_eq!(next_reply(&mut output).await[9], Status::BadStream as u8);
		drop(input);
		timeout(Duration::from_secs(2), task)
			.await
			.unwrap()
			.unwrap()
			.unwrap();
	}

	#[tokio::test]
	async fn keepalive_requires_negotiation_and_exact_version() {
		let (_mux, task, registry, input, _output) = mux(false).await;
		input.send(request(1, 1, 0, 0, 1)).unwrap();
		assert!(matches!(
			timeout(Duration::from_secs(2), task)
				.await
				.unwrap()
				.unwrap(),
			Err(WispError::InvalidPacketType(REQUEST_PACKET))
		));
		assert!(registry.0.lock().unwrap().is_empty());
		let mut builder = KeepaliveBuilder(SocketRegistry::default());
		for version in [
			Bytes::new(),
			Bytes::from_static(&[2]),
			Bytes::from_static(&[1, 0]),
		] {
			assert!(builder.build_from_bytes(version, Role::Server).is_err());
		}
	}
}
