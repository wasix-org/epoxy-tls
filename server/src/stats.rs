use std::collections::HashMap;

use serde::Serialize;
use wisp_mux::packet::{ConnectPacket, StreamType};

use crate::{CLIENTS, CONFIG};

fn format_stream_type(stream_type: StreamType) -> &'static str {
	match stream_type {
		StreamType::Tcp => "tcp",
		StreamType::Udp => "udp",
		#[cfg(feature = "twisp")]
		StreamType::Other(crate::handle::wisp::twisp::STREAM_TYPE) => "twisp",
		StreamType::Other(_) => unreachable!(),
	}
}

#[derive(Serialize)]
struct MemoryStats {
	active: usize,
	allocated: usize,
	mapped: usize,
	metadata: usize,
	resident: usize,
	retained: usize,
}

#[derive(Serialize)]
struct StreamStats {
	stream_type: String,
	requested: String,
	resolved: String,
}

impl From<(ConnectPacket, ConnectPacket)> for StreamStats {
	fn from(value: (ConnectPacket, ConnectPacket)) -> Self {
		Self {
			stream_type: format_stream_type(value.0.stream_type).to_string(),
			requested: format!("{}:{}", value.0.host, value.0.port),
			resolved: format!("{}:{}", value.1.host, value.1.port),
		}
	}
}

#[derive(Serialize)]
struct ClientStats {
	client_type: String,
	streams: HashMap<String, StreamStats>,
}

#[derive(Serialize)]
struct ServerStats {
	config: String,
	clients: HashMap<String, ClientStats>,
	memory: MemoryStats,
}

pub async fn generate_stats() -> anyhow::Result<String> {
	use tikv_jemalloc_ctl::stats::{active, allocated, mapped, metadata, resident, retained};
	tikv_jemalloc_ctl::epoch::advance()?;

	let memory = MemoryStats {
		active: active::read()?,
		allocated: allocated::read()?,
		mapped: mapped::read()?,
		metadata: metadata::read()?,
		resident: resident::read()?,
		retained: retained::read()?,
	};

	let clients_locked = CLIENTS.lock().await;

	let mut clients = HashMap::with_capacity(clients_locked.len());
	for client in clients_locked.iter() {
		clients.insert(
			client.0.to_string(),
			ClientStats {
				client_type: client.1 .1.clone(),
				streams: client
					.1
					 .0
					.lock()
					.await
					.iter()
					.map(|x| (x.0.to_string(), StreamStats::from(x.1.clone())))
					.collect(),
			},
		);
	}

	drop(clients_locked);

	let stats = ServerStats {
		config: CONFIG.ser()?,
		clients,
		memory,
	};

	Ok(serde_json::to_string_pretty(&stats)?)
}
