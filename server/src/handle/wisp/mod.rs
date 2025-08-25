#[cfg(feature = "twisp")]
pub mod twisp;
pub mod utils;
pub mod wispnet;

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use cfg_if::cfg_if;
use event_listener::Event;
use futures_util::{future::Either, FutureExt, SinkExt, StreamExt};
use log::{info, error, debug, trace};
use tokio::{
	io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
	select,
	task::JoinSet,
	time::interval,
};
use tokio_util::compat::{FuturesAsyncReadCompatExt, FuturesAsyncWriteCompatExt};
use uuid::Uuid;
use wisp_mux::{
	packet::{CloseReason, ConnectPacket},
	stream::MuxStream,
	ServerMux,
};
use wispnet::route_wispnet;

use crate::{
	route::{WispResult, WispStreamWrite, WispWsStreamWrite},
	stream::{ClientStream, ResolvedPacket},
	CLIENTS, CONFIG,
};


async fn manual_copy_buf<R, W>(
    mut reader: R,
    mut writer: W,
    direction: &'static str,
    buffer_size: usize,
) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0; buffer_size];
    let mut bytes_copied = 0;

    loop {
        // Attempt to read from the reader
        let read_result = reader.read(&mut buffer).await;
        let n = match read_result {
            Ok(0) => {
                // Reader returned 0 bytes, indicating EOF
                break; // Exit the loop on EOF
            },
            Ok(n) => {
                n
            }, // Successfully read `n` bytes
            Err(e) => {
                // An error occurred during reading
                error!("{}: Reader: Error reading: {:?}", direction, e);
                return Err(e); // Propagate the read error
            }
        };

        // Attempt to write the read data to the writer
        let write_result = writer.write_all(&buffer[..n]).await;
        if let Err(e) = write_result {
            // An error occurred during writing
            error!("{}: Writer: Error writing: {:?}", direction, e);
            return Err(e); // Propagate the write error
        }


        // Attempt to flush the writer to ensure data is sent
        let flush_result = writer.flush().await;
        if let Err(e) = flush_result {
            // An error occurred during flushing
            error!("{}: Writer: Error flushing: {:?}", direction, e);
            return Err(e); // Propagate the flush error
        }


        bytes_copied += n as u64;
    }

    info!("{}: Manual copy operation finished. Total bytes copied: {}", direction, bytes_copied);
    Ok(bytes_copied)
}

/// The main `copy_fast` function, now using `manual_copy_buf` for both directions.
/// This function sets up two concurrent data transfer tasks.
async fn copy_fast(
    mux: MuxStream<WispStreamWrite>,
    tcprx: impl AsyncRead + Unpin,
    mut tcptx: impl AsyncWrite + Unpin,
    #[cfg(feature = "speed-limit")] read_limit: async_speed_limit::Limiter<
        async_speed_limit::clock::StandardClock,
    >,
    #[cfg(feature = "speed-limit")] write_limit: async_speed_limit::Limiter<
        async_speed_limit::clock::StandardClock,
    >,
) -> std::io::Result<()> {
    // Split the MuxStream into its read and write halves
    let (muxrx_raw, muxtx_raw) = mux.into_async_rw().into_split();
    let mut muxrx = muxrx_raw.compat(); // Adapt to tokio::io::AsyncRead
    let mut muxtx = muxtx_raw.compat_write(); // Adapt to tokio::io::AsyncWrite

    // Apply speed limits if the feature is enabled
    #[cfg(feature = "speed-limit")]
    let tcprx = read_limit.limit(tcprx);
    #[cfg(feature = "speed-limit")]
    let mut tcptx = write_limit.limit(tcptx);

    // Buffer the TCP read stream for efficiency
    let mut tcprx_buf = BufReader::with_capacity(CONFIG.stream.buffer_size, tcprx);

    info!("Starting concurrent copy operations for Mux <-> TCP.");

    // Concurrently copy data in both directions using the manual_copy_buf function.
    // The `x?` operator will propagate any `std::io::Error` from either task.
    select! {
        x = manual_copy_buf(&mut muxrx, &mut tcptx, "Mux to TCP", CONFIG.stream.buffer_size) => {
            x.map(|_| info!("Mux to TCP copy completed."))
        },
        x = manual_copy_buf(&mut tcprx_buf, &mut muxtx, "TCP to Mux", CONFIG.stream.buffer_size) => {
            x.map(|_| info!("TCP to Mux copy completed."))
        },
    }?; // Propagate the first error or complete successfully

    info!("All copy operations completed successfully.");
    Ok(())
}

async fn resolve_stream(
	connect: ConnectPacket,
	muxstream: &MuxStream<WispStreamWrite>,
) -> Option<(ConnectPacket, ConnectPacket, ClientStream)> {
	let requested_stream = connect.clone();

	let Ok(resolved) = ClientStream::resolve(connect).await else {
		let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
		return None;
	};
	let (stream, resolved_stream) = match resolved {
		ResolvedPacket::Valid(connect) => {
			let resolved = connect.clone();
			let Ok(stream) = ClientStream::connect(connect).await else {
				let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
				return None;
			};
			(stream, resolved)
		}
		ResolvedPacket::ValidWispnet(server, connect) => {
			let resolved = connect.clone();
			let Ok(stream) = route_wispnet(server, connect).await else {
				let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
				return None;
			};
			(stream, resolved)
		}
		ResolvedPacket::NoResolvedAddrs => {
			let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
			return None;
		}
		ResolvedPacket::Blocked => {
			let _ = muxstream
				.close(CloseReason::ServerStreamBlockedAddress)
				.await;
			return None;
		}
		ResolvedPacket::Invalid => {
			let _ = muxstream.close(CloseReason::ServerStreamInvalidInfo).await;
			return None;
		}
	};

	Some((requested_stream, resolved_stream, stream))
}

async fn forward_stream(
	muxstream: MuxStream<WispStreamWrite>,
	stream: ClientStream,
	resolved_stream: ConnectPacket,
	id: &str,
	uuid: Uuid,
	#[cfg(feature = "twisp")] twisp_map: twisp::TwispMap,
	#[cfg(feature = "speed-limit")] read_limit: async_speed_limit::Limiter<
		async_speed_limit::clock::StandardClock,
	>,
	#[cfg(feature = "speed-limit")] write_limit: async_speed_limit::Limiter<
		async_speed_limit::clock::StandardClock,
	>,
) {
	match stream {
		ClientStream::Tcp(stream) => {
			let closer = muxstream.get_close_handle();

			trace!("[{id}] [{uuid}] [forward] starting tcp copy_fast");
			let (rx, tx) = stream.into_split();
			let ret: anyhow::Result<()> = async {
				copy_fast(
					muxstream,
					rx,
					tx,
					#[cfg(feature = "speed-limit")]
					read_limit,
					#[cfg(feature = "speed-limit")]
					write_limit,
				)
				.await?;
				Ok(())
			}
			.await;
			trace!("[{id}] [{uuid}] [forward] ended tcp copy_fast");

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
			let (mut read, write) = muxstream.into_split();
			let mut write = write.into_async_write().compat_write();

			trace!("[{id}] [{uuid}] [forward] starting udp copy");
			let ret: anyhow::Result<()> = async move {
				let mut data = vec![0u8; 65507];
				loop {
					select! {
						size = stream.recv(&mut data) => {
							let size = size?;
							write.write_all(&data[..size]).await?;
						}
						data = read.next() => {
							if let Some(data) = data.transpose()? {
								stream.send(&data).await?;
							} else {
								break Ok(());
							}
						}
					}
				}
			}
			.await;
			trace!("[{id}] [{uuid}] [forward] ended udp copy");

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
			let id = muxstream.get_stream_id();
			let (mut rx, mut tx) = muxstream.into_async_rw().into_split();

			match twisp::handle_twisp(id, &mut rx, &mut tx, twisp_map.clone(), pty, cmd).await {
				Ok(()) => {
					let _ = closer.close(CloseReason::Voluntary).await;
				}
				Err(_) => {
					let _ = closer.close(CloseReason::Unexpected).await;
				}
			}
		}
		ClientStream::Wispnet(stream, mux_id) => {
			Box::pin(wispnet::handle_stream(
				muxstream,
				stream,
				mux_id,
				uuid,
				resolved_stream,
				#[cfg(feature = "speed-limit")]
				read_limit,
				#[cfg(feature = "speed-limit")]
				write_limit,
			))
			.await;
		}
		ClientStream::NoResolvedAddrs => {
			trace!("[{id}] [{uuid}] [forward] no resolved addrs");
			let _ = muxstream.close(CloseReason::ServerStreamUnreachable).await;
		}
		ClientStream::Invalid => {
			trace!("[{id}] [{uuid}] [forward] invalid");
			let _ = muxstream.close(CloseReason::ServerStreamInvalidInfo).await;
		}
		ClientStream::Blocked => {
			trace!("[{id}] [{uuid}] [forward] blocked");
			let _ = muxstream
				.close(CloseReason::ServerStreamBlockedAddress)
				.await;
		}
	}
}

async fn handle_stream(
	connect: ConnectPacket,
	muxstream: MuxStream<WispStreamWrite>,
	id: String,
	event: Arc<Event>,
	#[cfg(feature = "twisp")] twisp_map: twisp::TwispMap,
	#[cfg(feature = "speed-limit")] read_limit: async_speed_limit::Limiter<
		async_speed_limit::clock::StandardClock,
	>,
	#[cfg(feature = "speed-limit")] write_limit: async_speed_limit::Limiter<
		async_speed_limit::clock::StandardClock,
	>,
) {
	let uuid = Uuid::new_v4();
	debug!("[{id}] [{uuid}] stream requested: {connect:?}");

	let Some((requested_stream, resolved_stream, stream)) = select! (
		x = resolve_stream(connect, &muxstream) => x,
		() = event.listen() => None
	) else {
		debug!("[{id}] [{uuid}] stream cancelled");
		// muxstream was closed
		return;
	};

	debug!("[{id}] [{uuid}] stream resolved and connected: {resolved_stream:?}");

	if let Some(client) = CLIENTS.lock().await.get(&id) {
		client
			.0
			.lock()
			.await
			.insert(uuid, (requested_stream, resolved_stream.clone()));
	}

	let forward_fut = forward_stream(
		muxstream,
		stream,
		resolved_stream,
		&id,
		uuid,
		#[cfg(feature = "twisp")]
		twisp_map,
		#[cfg(feature = "speed-limit")]
		read_limit,
		#[cfg(feature = "speed-limit")]
		write_limit,
	);

	select! {
		x = forward_fut => x,
		() = event.listen() => (),
	};

	debug!("[{id}] [{uuid}] stream disconnected");

	if let Some(client) = CLIENTS.lock().await.get(&id) {
		client.0.lock().await.remove(&uuid);
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

	#[cfg(feature = "speed-limit")]
	let read_limiter = async_speed_limit::Limiter::builder(CONFIG.wisp.read_limit)
		.refill(Duration::from_secs(1))
		.clock(async_speed_limit::clock::StandardClock)
		.build();
	#[cfg(feature = "speed-limit")]
	let write_limiter = async_speed_limit::Limiter::builder(CONFIG.wisp.write_limit)
		.refill(Duration::from_secs(1))
		.clock(async_speed_limit::clock::StandardClock)
		.build();

	let (mux, fut) = Box::pin(
		Box::pin(ServerMux::new(
			read,
			write,
			buffer_size,
			if is_v2 { extensions } else { None },
		))
		.await
		.context("failed to create server multiplexor")?
		.with_required_extensions(&required_extensions),
	)
	.await?;
	let mux = Arc::new(mux);

	debug!(
		"[{id}] client connected with extensions {:?}, downgraded {:?}",
		mux.get_extension_ids(),
		mux.was_downgraded()
	);

	let mut set: JoinSet<()> = JoinSet::new();
	let event: Arc<Event> = Event::new().into();

	let mux_id = id.clone();
	set.spawn(fut.map(move |x| debug!("[{mux_id}] multiplexor result: {x:?}")));

	let ping_mux = mux.clone();
	let ping_event = event.clone();
	let ping_id = id.clone();
	set.spawn(async move {
		let mut interval = interval(Duration::from_secs(30));
		let send_ping = || async {
			let mut locked = ping_mux.lock_ws().await?;
			if let Either::Left(ws) = &mut *locked {
				<WispWsStreamWrite as SinkExt<tokio_websockets::Message>>::send(
					ws,
					tokio_websockets::Message::ping(&[] as &[u8]),
				)
				.await?;
			}
			anyhow::Ok(())
		};

		while (send_ping)().await.is_ok() {
			trace!("[{ping_id}] sent ping");
			select! {
				_ = interval.tick() => (),
				() = ping_event.listen() => break,
			};
		}
	});

	while let Some((connect, stream)) = mux.wait_for_stream().await {
		set.spawn(handle_stream(
			connect,
			stream,
			id.clone(),
			event.clone(),
			#[cfg(feature = "twisp")]
			twisp_map.clone(),
			#[cfg(feature = "speed-limit")]
			read_limiter.clone(),
			#[cfg(feature = "speed-limit")]
			write_limiter.clone(),
		));
	}

	debug!("[{id}] shutting down wisp client");

	let _ = mux.close().await;
	event.notify(usize::MAX);

	trace!("[{id}] waiting for tasks to close");

	while set.join_next().await.is_some() {}

	debug!("[{id}] client disconnected");

	Ok(())
}
