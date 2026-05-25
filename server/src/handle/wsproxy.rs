use std::str::FromStr;

use anyhow::bail;
use futures_util::{SinkExt, StreamExt};
use log::debug;
use tokio::{
	io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
	select,
};
use tokio_websockets::CloseCode;
use uuid::Uuid;
use wisp_mux::packet::{CloseReason, ConnectPacket, StreamType};

use crate::{
	handle::wisp::wispnet::route_wispnet,
	stream::{ClientStream, ResolvedPacket, WebSocketFrame, WebSocketStreamWrapper},
	CLIENTS, CONFIG,
};

// TODO rewrite this whole thing
//      isn't even cancel safe i think
#[allow(clippy::too_many_lines)]
pub async fn handle_wsproxy(
	mut ws: WebSocketStreamWrapper,
	id: String,
	path: String,
	udp: bool,
) -> anyhow::Result<()> {
	if udp && !CONFIG.stream.allow_wsproxy_udp {
		let _ = ws
			.close(CloseCode::POLICY_VIOLATION, "udp is blocked")
			.await;
		return Ok(());
	}

	let Some(vec) = path
		.split('/')
		.next_back()
		.map(|x| x.split(':').collect::<Vec<_>>())
	else {
		let _ = ws.close(CloseCode::POLICY_VIOLATION, "invalid path").await;
		return Ok(());
	};
	let Some(host) = vec.first().map(ToString::to_string) else {
		let _ = ws.close(CloseCode::POLICY_VIOLATION, "invalid host").await;
		return Ok(());
	};
	let Some(port) = vec.get(1).and_then(|x| FromStr::from_str(x).ok()) else {
		let _ = ws.close(CloseCode::POLICY_VIOLATION, "invalid port").await;
		return Ok(());
	};

	let connect = ConnectPacket {
		stream_type: if udp {
			StreamType::Udp
		} else {
			StreamType::Tcp
		},
		host,
		port,
	};

	let requested_stream = connect.clone();

	let Ok(resolved) = ClientStream::resolve(connect).await else {
		let _ = ws
			.close(CloseCode::INTERNAL_SERVER_ERROR, "failed to resolve host")
			.await;
		return Ok(());
	};
	let (stream, resolved_stream) = match resolved {
		ResolvedPacket::Valid(connect) => {
			let resolved = connect.clone();
			let Ok(stream) = ClientStream::connect(connect).await else {
				let _ = ws
					.close(
						CloseCode::INTERNAL_SERVER_ERROR,
						"failed to connect to host",
					)
					.await;
				return Ok(());
			};
			(stream, resolved)
		}
		ResolvedPacket::ValidWispnet(server, connect) => {
			let resolved = connect.clone();
			let Ok(stream) = route_wispnet(server, connect).await else {
				let _ = ws
					.close(
						CloseCode::INTERNAL_SERVER_ERROR,
						"failed to connect to host",
					)
					.await;
				return Ok(());
			};
			(stream, resolved)
		}
		ResolvedPacket::NoResolvedAddrs => {
			let _ = ws
				.close(
					CloseCode::INTERNAL_SERVER_ERROR,
					"host did not resolve to any addrs",
				)
				.await;
			return Ok(());
		}
		ResolvedPacket::Blocked => {
			let _ = ws
				.close(CloseCode::POLICY_VIOLATION, "host is blocked")
				.await;
			return Ok(());
		}
		ResolvedPacket::Invalid => {
			let _ = ws
				.close(
					CloseCode::POLICY_VIOLATION,
					"invalid host/port/type combination",
				)
				.await;
			return Ok(());
		}
	};

	let uuid = Uuid::new_v4();

	debug!(
		"new wsproxy client id {:?} connected: (stream uuid {:?}) {:?} {:?}",
		id, uuid, requested_stream, resolved_stream
	);

	if let Some(client) = CLIENTS.lock().await.get(&id) {
		client
			.0
			.lock()
			.await
			.insert(uuid, (requested_stream, resolved_stream.clone()));
	}

	match stream {
		ClientStream::Tcp(stream) => {
			let mut stream = BufReader::new(stream);
			let ret: anyhow::Result<()> = async {
				loop {
					select! {
						x = ws.read() => {
							match x.transpose()? {
								Some(WebSocketFrame::Data(data)) => {
									stream.write_all(&data).await?;
								}
								Some(WebSocketFrame::Close) => {
									stream.shutdown().await?;
								}
								Some(WebSocketFrame::Ignore) => {}
								None => break Ok(()),
							}
						}
						x = stream.fill_buf() => {
							let x = x?;
							ws.write(x.to_vec()).await?;
							let len = x.len();
							stream.consume(len);
						}
					}
				}
			}
			.await;
			match ret {
				Ok(()) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, "").await;
				}
				Err(x) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, &x.to_string()).await;
				}
			}
		}
		ClientStream::Udp(stream) => {
			let ret: anyhow::Result<()> = async {
				let mut data = vec![0u8; 65507];
				loop {
					select! {
						x = ws.read() => {
							match x.transpose()? {
								Some(WebSocketFrame::Data(data)) => {
									stream.send(&data).await?;
								}
								Some(WebSocketFrame::Close | WebSocketFrame::Ignore) => {}
								None => break Ok(()),
							}
						}
						size = stream.recv(&mut data) => {
							ws.write(data[..size?].to_vec()).await?;
						}
					}
				}
			}
			.await;
			match ret {
				Ok(()) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, "").await;
				}
				Err(x) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, &x.to_string()).await;
				}
			}
		}
		ClientStream::UdpSocks5(stream, target) => {
			let ret: anyhow::Result<()> = async {
				let mut data = vec![0u8; 65507];
				loop {
					select! {
						x = ws.read() => {
							match x.transpose()? {
								Some(WebSocketFrame::Data(data)) => {
									stream.send_to(&data, target.clone()).await?;
								}
								Some(WebSocketFrame::Close | WebSocketFrame::Ignore) => {}
								None => break Ok(()),
							}
						}
						size = stream.recv_from(&mut data) => {
							let (size, new_target) = size?;
							if new_target != target {
								bail!("target changed while forwarding udp to {target} over socks5");
							}
							ws.write(data[..size].to_vec()).await?;
						}
					}
				}
			}
			.await;
			match ret {
				Ok(()) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, "").await;
				}
				Err(x) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, &x.to_string()).await;
				}
			}
		}
		#[cfg(feature = "twisp")]
		ClientStream::Pty(_, _) => {
			let _ = ws
				.close(CloseCode::POLICY_VIOLATION, "twisp is not supported")
				.await;
		}
		ClientStream::Wispnet(mut stream, mux_id) => {
			if let Some(client) = CLIENTS.lock().await.get(&mux_id) {
				client
					.0
					.lock()
					.await
					.insert(uuid, (resolved_stream.clone(), resolved_stream));
			}

			let ret: anyhow::Result<()> = async {
				loop {
					select! {
						x = ws.read() => {
							match x.transpose()? {
								Some(WebSocketFrame::Data(data)) => {
									stream.send(data.into()).await?;
								}
								Some(WebSocketFrame::Close) => {
									stream.close(CloseReason::Voluntary).await?;
								}
								Some(WebSocketFrame::Ignore) => {}
								None => break,
							}
						}
						x = stream.next() => {
							let Some(x) = x else {
								break;
							};
							ws.write(x?).await?;
						}
					}
				}
				Ok(())
			}
			.await;

			if let Some(client) = CLIENTS.lock().await.get(&mux_id) {
				client.0.lock().await.remove(&uuid);
			}

			match ret {
				Ok(()) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, "").await;
				}
				Err(x) => {
					let _ = ws.close(CloseCode::NORMAL_CLOSURE, &x.to_string()).await;
				}
			}
		}
		ClientStream::NoResolvedAddrs => {
			let _ = ws
				.close(
					CloseCode::INTERNAL_SERVER_ERROR,
					"host did not resolve to any addrs",
				)
				.await;
			return Ok(());
		}
		ClientStream::Blocked => {
			let _ = ws
				.close(CloseCode::POLICY_VIOLATION, "host is blocked")
				.await;
		}
		ClientStream::Invalid => {
			let _ = ws
				.close(CloseCode::POLICY_VIOLATION, "host is invalid")
				.await;
		}
	}

	debug!(
		"wsproxy client id {:?} disconnected (stream uuid {:?})",
		id, uuid
	);

	if let Some(client) = CLIENTS.lock().await.get(&id) {
		client.0.lock().await.remove(&uuid);
	}

	Ok(())
}
