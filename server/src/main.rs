#![doc(html_no_source)]
#![deny(clippy::todo)]
#![allow(unexpected_cfgs)]

use std::{collections::HashMap, fs::read_to_string, net::IpAddr};

use anyhow::{Context, Result};
use clap::Parser;
use config::{validate_config_cache, Cli, Config, RuntimeFlavor, StatsEndpoint};
use handle::{handle_wisp, handle_wsproxy, wisp::wispnet::handle_wispnet};
use hickory_resolver::{
	config::{NameServerConfigGroup, ResolverConfig, ResolverOpts},
	system_conf::read_system_conf,
	TokioAsyncResolver,
};
use lazy_static::lazy_static;
use listener::ServerListener;
use log::{error, info, trace, warn};
use route::{route_stats, ServerRouteResult};
use stats::generate_stats;
use tokio::{
	runtime,
	signal::unix::{signal, SignalKind},
	sync::Mutex,
};
use uuid::Uuid;
use wisp_mux::packet::ConnectPacket;

pub mod config;
#[doc(hidden)]
mod handle;
#[doc(hidden)]
mod listener;
#[doc(hidden)]
mod route;
#[doc(hidden)]
mod stats;
#[doc(hidden)]
mod stream;
#[doc(hidden)]
mod upgrade;
#[doc(hidden)]
mod util_chain;
#[doc(hidden)]
mod util_map_err;

#[doc(hidden)]
type Client = (Mutex<HashMap<Uuid, (ConnectPacket, ConnectPacket)>>, String);

#[doc(hidden)]
#[derive(Debug)]
pub enum Resolver {
	Hickory(TokioAsyncResolver),
	System,
}

impl Resolver {
	pub async fn resolve(&self, host: String) -> anyhow::Result<Box<dyn Iterator<Item = IpAddr>>> {
		match self {
			Self::Hickory(resolver) => Ok(Box::new(resolver.lookup_ip(host).await?.into_iter())),
			Self::System => Ok(Box::new(
				tokio::net::lookup_host(host + ":0").await?.map(|x| x.ip()),
			)),
		}
	}

	pub fn clear_cache(&self) {
		match self {
			Self::Hickory(resolver) => resolver.clear_cache(),
			Self::System => {}
		}
	}
}

lazy_static! {
	#[doc(hidden)]
	pub static ref CLI: Cli = Cli::parse();
	#[doc(hidden)]
	pub static ref CONFIG: Config = {
		if let Some(path) = &CLI.config {
			Config::de(
				&read_to_string(path)
					.context("failed to read config")
					.unwrap(),
			)
			.context("failed to parse config")
			.unwrap()
		} else {
			Config::default()
		}
	};
	#[doc(hidden)]
	pub static ref CLIENTS: Mutex<HashMap<String, Client>> = Mutex::new(HashMap::new());
	#[doc(hidden)]
	pub static ref RESOLVER: Resolver = {
		if CONFIG.stream.dns_servers.is_empty() {
			if let Ok((config, opts)) = read_system_conf() {
				Resolver::Hickory(TokioAsyncResolver::tokio(config, opts))
			} else {
				warn!("unable to read system dns configuration. using system dns resolver with no caching");
				Resolver::System
			}
		} else {
			Resolver::Hickory(TokioAsyncResolver::tokio(
				ResolverConfig::from_parts(
					None,
					Vec::new(),
					NameServerConfigGroup::from_ips_clear(&CONFIG.stream.dns_servers, 53, true),
				),
				ResolverOpts::default(),
			))
		}
	};
}

#[doc(hidden)]
#[global_allocator]
static JEMALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[doc(hidden)]
fn main() -> Result<()> {
	if CLI.default_config {
		println!("{}", Config::default().ser()?);
		return Ok(());
	}

	env_logger::builder()
		.filter_level(CONFIG.server.log_level)
		.parse_default_env()
		.init();

	let mut builder: runtime::Builder = match CONFIG.server.runtime {
		RuntimeFlavor::SingleThread => runtime::Builder::new_current_thread(),
		RuntimeFlavor::MultiThread => runtime::Builder::new_multi_thread(),
		#[cfg(tokio_unstable)]
		RuntimeFlavor::MultiThreadAlt => runtime::Builder::new_multi_thread_alt(),
	};

	builder.enable_all();
	let rt = builder.build()?;

	rt.block_on(async move {
		tokio::spawn(async_main()).await??;
		Ok(())
	})
}

#[doc(hidden)]
async fn async_main() -> Result<()> {
	#[cfg(feature = "tokio-console")]
	console_subscriber::init();

	validate_config_cache().await;

	info!(
		"listening on {:?} with runtime flavor {:?} and socket transport {:?}",
		CONFIG.server.bind, CONFIG.server.runtime, CONFIG.server.transport
	);

	trace!("CLI: {:#?}", &*CLI);
	trace!("CONFIG: {:#?}", &*CONFIG);
	trace!("RESOLVER: {:?}", &*RESOLVER);

	tokio::spawn(async {
		let mut sig = signal(SignalKind::user_defined1()).unwrap();
		while sig.recv().await.is_some() {
			match generate_stats().await {
				Ok(stats) => info!("Stats:\n{}", stats),
				Err(err) => error!("error while creating stats {:?}", err),
			}
		}
	});

	let mut listener = ServerListener::new(&CONFIG.server.bind)
		.await
		.with_context(|| format!("failed to bind to address {}", CONFIG.server.bind.1))?;

	if let Some(bind_addr) = CONFIG
		.server
		.stats_endpoint
		.as_ref()
		.and_then(StatsEndpoint::get_bindaddr)
	{
		info!("stats server listening on {:?}", bind_addr);
		let mut stats_listener = ServerListener::new(&bind_addr).await.with_context(|| {
			format!("failed to bind to address {} for stats server", bind_addr.1)
		})?;

		tokio::spawn(async move {
			loop {
				match stats_listener.accept().await {
					Ok((stream, _)) => {
						tokio::spawn(async move {
							if let Err(e) = Box::pin(route_stats(stream)).await {
								error!("error while routing stats client: {:?}", e);
							}
						});
					}
					Err(e) => error!("error while accepting stats client: {:?}", e),
				}
			}
		});
	}

	let stats_endpoint = CONFIG
		.server
		.stats_endpoint
		.as_ref()
		.and_then(StatsEndpoint::get_endpoint);

	loop {
		let stats_endpoint = stats_endpoint.clone();
		match listener.accept().await {
			Ok((stream, client_id)) => {
				tokio::spawn(async move {
					let res = Box::pin(route::route(
						stream,
						stats_endpoint,
						move |stream, maybe_ip| {
							let client_id = if let Some(ip) = maybe_ip {
								format!("{client_id} ({ip})")
							} else {
								client_id
							};

							trace!("routed {:?}: {}", client_id, stream);
							handle_stream(stream, client_id);
						},
					))
					.await;

					if let Err(e) = res {
						error!("error while routing client: {:?}", e);
					}
				});
			}
			Err(e) => error!("error while accepting client: {:?}", e),
		}
	}
}

#[doc(hidden)]
fn handle_stream(stream: ServerRouteResult, id: String) {
	tokio::spawn(async move {
		CLIENTS.lock().await.insert(
			id.clone(),
			(Mutex::new(HashMap::new()), format!("{stream}")),
		);
		let res = match stream {
			ServerRouteResult::Wisp {
				stream,
				has_ws_protocol,
			} => Box::pin(handle_wisp(stream, has_ws_protocol, id.clone())).await,
			ServerRouteResult::Wispnet { stream } => {
				Box::pin(handle_wispnet(stream, id.clone())).await
			}
			ServerRouteResult::WsProxy { stream, path, udp } => {
				Box::pin(handle_wsproxy(stream, id.clone(), path, udp)).await
			}
		};
		if let Err(e) = res {
			error!("error while handling client: {:?}", e);
		}
		CLIENTS.lock().await.remove(&id)
	});
}
