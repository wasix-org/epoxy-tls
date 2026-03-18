use std::{
	error::Error as StdError,
	io,
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
	time::Duration,
};

use bytes::Bytes;
use futures::{StreamExt, lock::Mutex};
use http::Uri;
use http_body::{Body, Frame, SizeHint};
use http_body_util::{Either, Empty, StreamBody};
use hyper::{Request, Response, body::Incoming, rt::Executor};
use hyper_util::client::pool::{self, cache, negotiate, singleton};
use js_sys::Uint8Array;
use tower::{
	Service, ServiceExt, service_fn,
	util::{BoxCloneService, BoxService},
};
use tower_layer::Layer;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{EpoxyError, send_wrapper::SendWrapper};

use super::StreamProviderService;

pub mod conn;
pub mod expire;

#[derive(Clone)]
pub struct WasmExecutor;
impl<T: Future<Output = ()> + 'static> Executor<T> for WasmExecutor {
	fn execute(&self, fut: T) {
		spawn_local(fut);
	}
}

pub enum EpoxyBody {
	Empty,
	Stream {
		current: SendWrapper<wasm_streams::readable::IntoStream<'static>>,
		tee: web_sys::ReadableStream,
		length: Option<u64>,
	},
}
impl EpoxyBody {
	pub fn new(stream: web_sys::ReadableStream, length: Option<u64>) -> Self {
		let (a, b) = wasm_streams::ReadableStream::from_raw(stream).tee();

		Self::Stream {
			current: SendWrapper(a.into_stream()),
			tee: b.into_raw(),
			length,
		}
	}
}
impl Body for EpoxyBody {
	type Data = Bytes;
	type Error = EpoxyError;

	fn poll_frame(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
	) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
		match &mut *self {
			Self::Empty => Poll::Ready(None),
			Self::Stream { current, .. } => current
				.0
				.poll_next_unpin(cx)
				.map_ok(|x| {
					let u8array = x.unchecked_into::<Uint8Array>();
					Frame::data(Bytes::from(u8array.to_vec()))
				})
				.map_err(EpoxyError::js_error),
		}
	}

	fn is_end_stream(&self) -> bool {
		matches!(self, Self::Empty)
	}

	fn size_hint(&self) -> SizeHint {
		match self {
			Self::Empty => SizeHint::with_exact(0),
			Self::Stream { length, .. } => {
				length.map(|x| SizeHint::with_exact(x)).unwrap_or_default()
			}
		}
	}
}
impl Default for EpoxyBody {
	fn default() -> Self {
		Self::Empty
	}
}
impl Clone for EpoxyBody {
	fn clone(&self) -> Self {
		match self {
			Self::Empty => Self::Empty,
			Self::Stream {
				current: _,
				tee,
				length,
			} => {
				let (a, b) = wasm_streams::ReadableStream::from_raw(tee.clone()).tee();

				Self::Stream {
					current: SendWrapper(a.into_stream()),
					tee: b.into_raw(),
					length: *length,
				}
			}
		}
	}
}

type HyperRequest = Request<EpoxyBody>;
type HyperResponse = Response<Incoming>;
type HyperResponseFuture<T> = Pin<Box<dyn Future<Output = Result<T, EpoxyError>> + Send>>;

pub type HyperRequestService = BoxService<HyperRequest, HyperResponse, EpoxyError>;

pub(super) type BoxError = Box<dyn StdError + Send + Sync + 'static>;
pub type HyperClient = BoxCloneService<Uri, HyperRequestService, EpoxyError>;

type OriginKey = (Option<http::uri::Scheme>, Option<http::uri::Authority>);

#[derive(Clone)]
struct PrunedPool<P> {
	inner: P,
	prune: fn(&mut P),
}

impl<P> PrunedPool<P> {
	fn new(inner: P, prune: fn(&mut P)) -> Self {
		Self { inner, prune }
	}

	fn prune(&mut self) {
		(self.prune)(&mut self.inner);
	}
}

impl<P, Req> Service<Req> for PrunedPool<P>
where
	P: Service<Req>,
{
	type Response = P::Response;
	type Error = P::Error;
	type Future = P::Future;

	fn poll_ready(
		&mut self,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		self.prune();
		self.inner.poll_ready(cx)
	}

	fn call(&mut self, req: Req) -> Self::Future {
		self.prune();
		self.inner.call(req)
	}
}

#[derive(Clone)]
struct UriPoolService<P> {
	pool: P,
}

impl<P> Service<Uri> for UriPoolService<P>
where
	P: Service<Uri> + Clone + Send + 'static,
	P::Future: Send + 'static,
	P::Error: Into<BoxError>,
	P::Response: Service<HyperRequest, Response = HyperResponse, Error = BoxError> + Send + 'static,
	<P::Response as Service<HyperRequest>>::Future: Send + 'static,
{
	type Response = HyperRequestService;
	type Error = EpoxyError;
	type Future = HyperResponseFuture<Self::Response>;

	fn poll_ready(
		&mut self,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), Self::Error>> {
		self.pool
			.poll_ready(cx)
			.map_err(|err| pool_error(err.into()))
	}

	fn call(&mut self, uri: Uri) -> Self::Future {
		let fut = self.pool.call(uri);
		Box::pin(async move {
			let selected = fut.await.map_err(|err| pool_error(err.into()))?;
			Ok(BoxService::new(selected.map_err(pool_error)))
		})
	}
}

pub fn build_hyper_client(provider: StreamProviderService) -> HyperClient {
	let pools = pool::map::Map::builder::<Uri>()
		.keys(scheme_and_auth)
		.values(move |_| build_origin_service(provider.clone()))
		.build();

	let pools = Arc::new(Mutex::new(pools));

	BoxCloneService::new(service_fn(move |uri: Uri| {
		let pools = pools.clone();
		async move {
			let pooled = {
				let mut pools = pools.lock().await;
				pools.service(&uri).clone()
			};

			pooled.oneshot(uri).await
		}
	}))
}

fn build_origin_service(provider: StreamProviderService) -> HyperClient {
	let expire = expire::ExpireConfig {
		idle_timeout: Duration::from_secs(90),
		max_lifetime: Duration::from_secs(300),
	};
	let http1 = tower::layer::layer_fn(move |svc| {
		let svc = conn::http1::<_, EpoxyBody>(expire).layer(svc);
		let svc = cache::builder().executor(WasmExecutor).build(svc);
		PrunedPool::new(svc, |pool| pool.retain(|service| !service.is_expired()))
	});
	let http2 = tower::layer::layer_fn(move |svc| {
		let svc = conn::http2::<_, EpoxyBody>(expire).layer(svc);
		let svc = singleton::Singleton::new(svc);
		PrunedPool::new(svc, |pool| pool.retain(|service| !service.is_expired()))
	});
	let pool = negotiate::builder()
		.connect(provider)
		.inspect(|conn: &super::HttpIo| conn.is_negotiated_h2())
		.fallback(http1)
		.upgrade(http2)
		.build::<Uri>();

	BoxCloneService::new(UriPoolService { pool })
}

fn pool_error(err: BoxError) -> EpoxyError {
	let err = match err.downcast::<EpoxyError>() {
		Ok(err) => return *err,
		Err(err) => err,
	};

	let err = match err.downcast::<io::Error>() {
		Ok(err) => return EpoxyError::from(*err),
		Err(err) => err,
	};

	let err = match err.downcast::<hyper::Error>() {
		Ok(err) => return EpoxyError::from(*err),
		Err(err) => err,
	};

	let err = match err.downcast::<hyper::http::Error>() {
		Ok(err) => return EpoxyError::from(*err),
		Err(err) => err,
	};

	if find_source::<expire::ConnectionExpired>(&*err).is_some() {
		return io::Error::new(io::ErrorKind::TimedOut, err.to_string()).into();
	}

	if let Some(err) = find_source::<io::Error>(&*err) {
		return io::Error::new(err.kind(), err.to_string()).into();
	}

	io::Error::other(err.to_string()).into()
}

fn find_source<'a, T>(err: &'a (dyn StdError + 'static)) -> Option<&'a T>
where
	T: StdError + 'static,
{
	if let Some(err) = err.downcast_ref::<T>() {
		return Some(err);
	}

	err.source().and_then(find_source::<T>)
}

fn scheme_and_auth(uri: &Uri) -> OriginKey {
	(uri.scheme().cloned(), uri.authority().cloned())
}
