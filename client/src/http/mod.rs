use std::{
	error::Error as StdError,
	future::Future,
	io,
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
};

use bytes::Bytes;
use futures::{StreamExt, lock::Mutex};
use http::Uri;
use http_body::{Body, Frame, SizeHint};
use hyper::{Request, Response, body::Incoming, rt::Executor};
use hyper_util::client::pool::{cache, map, negotiate, singleton};
use js_sys::Uint8Array;
use tower::{Service, ServiceBuilder, ServiceExt, service_fn, util::MapResponseLayer};
use tower_layer::layer_fn;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::{
	EpoxyError,
	provider::{HttpIo, StreamProviderService},
	send_wrapper::SendWrapper,
};

mod conn;
mod decomp;
mod h1;

#[derive(Clone)]
pub struct WasmExecutor;
impl<T: Future<Output = ()> + 'static> Executor<T> for WasmExecutor {
	fn execute(&self, fut: T) {
		spawn_local(fut);
	}
}

struct DelayedRelease<S> {
	inner: Option<S>,
}

impl<S> DelayedRelease<S> {
	fn new(inner: S) -> Self {
		Self { inner: Some(inner) }
	}
}

impl<Req, S> Service<Req> for DelayedRelease<S>
where
	S: Service<Req> + Send + 'static,
	S::Future: Send + 'static,
{
	type Response = S::Response;
	type Error = S::Error;
	type Future =
		Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner
			.as_mut()
			.expect("DelayedRelease polled after inner service was consumed")
			.poll_ready(cx)
	}

	fn call(&mut self, req: Req) -> Self::Future {
		let mut inner = self
			.inner
			.take()
			.expect("DelayedRelease called more than once");
		let fut = inner.call(req);

		Box::pin(async move {
			let result = fut.await;
			spawn_local(async move {
				let _ = ServiceExt::ready(&mut inner).await;
			});
			result
		})
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

pub(super) type BoxError = Box<dyn StdError + Send + Sync + 'static>;

type OriginKey = (Option<http::uri::Scheme>, Option<http::uri::Authority>);

pub type HyperRequest = Request<EpoxyBody>;
pub type HyperResponse = Response<Incoming>;
pub type HyperClient =
	impl Service<HyperRequest, Response = HyperResponse, Error = EpoxyError> + Clone;

#[define_opaque(HyperClient)]
pub fn build_hyper_client(provider: StreamProviderService) -> HyperClient {
	let pools = map::Map::builder::<Uri>()
		.keys(scheme_and_auth)
		.values(move |_| build_origin_service(provider.clone()))
		.build();
	let pools = Arc::new(Mutex::new(pools));

	service_fn(move |req: HyperRequest| {
		let pools = pools.clone();
		async move {
			let uri = req.uri().clone();
			let pooled = {
				let mut pools = pools.lock().await;
				pools.service(&uri).clone()
			};

			let client = pooled.oneshot(uri).await?;
			client.oneshot(req).await
		}
	})
}

type OriginServiceRet = impl Service<
		HyperRequest,
		Response = HyperResponse,
		Error = EpoxyError,
		Future = impl Future<Output = Result<HyperResponse, EpoxyError>> + Send,
	> + Send;
#[define_opaque(OriginServiceRet)]
fn build_origin_service(
	provider: StreamProviderService,
) -> impl Service<
	Uri,
	Response = OriginServiceRet,
	Error = EpoxyError,
	Future = impl Future<Output = Result<OriginServiceRet, EpoxyError>> + Send,
> + Clone {
	let http1 = layer_fn(move |svc| {
		ServiceBuilder::new()
			.layer(MapResponseLayer::new(|x| {
				ServiceBuilder::new()
					.layer_fn(DelayedRelease::new)
					.layer_fn(h1::SetHost::new)
					.layer_fn(h1::RequestTarget::new)
					.service(x)
			}))
			.layer_fn(|x| cache::builder().executor(WasmExecutor).build(x))
			.layer(conn::http1())
			.service(svc)
	});
	let http2 = layer_fn(move |svc| {
		ServiceBuilder::new()
			.layer_fn(singleton::Singleton::new)
			.layer(conn::http2())
			.service(svc)
	});
	let pool = negotiate::builder()
		.connect(provider)
		.inspect(|conn: &HttpIo| conn.is_negotiated_h2())
		.fallback(http1)
		.upgrade(http2)
		.build::<Uri>();

	pool.map_response(|client| client.map_err(pool_error))
		.map_err(|x| pool_error(x.into()))
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
