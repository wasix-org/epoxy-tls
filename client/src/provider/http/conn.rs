use std::{
	error::Error as StdError,
	future::Future,
	pin::Pin,
	task::{Context, Poll},
};

use http::{Request, Response, Uri};
use hyper::{body::Incoming, client::conn, rt::Executor};
use tower::{Service, ServiceExt, service_fn, util::BoxCloneService};
use tower_layer::Layer;

use crate::console_log;

use super::{
	BoxError, WasmExecutor,
	expire::{Expire, ExpireConfig},
};

type ConnFuture<T> = Pin<Box<dyn Future<Output = Result<T, BoxError>> + Send>>;

pub(super) struct Http1Send<B>(hyper::client::conn::http1::SendRequest<B>);
pub(super) struct Http2Send<B>(hyper::client::conn::http2::SendRequest<B>);

impl<B> Clone for Http2Send<B> {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

pub(super) fn http1<S, B>(
	expire: ExpireConfig,
) -> impl Layer<S, Service = BoxCloneService<Uri, Expire<Http1Send<B>>, BoxError>> + Clone
where
	S: Service<Uri, Response = super::super::HttpIo, Error = BoxError> + Clone + Send + 'static,
	S::Future: Send + 'static,
	B: hyper::body::Body + Send + Unpin + 'static,
	B::Data: Send,
	B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
	tower::layer::layer_fn(move |connector: S| {
		BoxCloneService::new(service_fn(move |dst: Uri| {
			let connector = connector.clone();
			Box::pin(async move {
				let io = connector.oneshot(dst).await?;
				let mut builder = conn::http1::Builder::new();
				builder.http09_responses(true).preserve_header_case(true);
				let (tx, conn) = builder
					.handshake(io)
					.await
					.map_err(|err| -> BoxError { Box::new(err) })?;
				WasmExecutor.execute(async move {
					let _ = conn.with_upgrades().await;
				});

				Ok(Expire::new(Http1Send(tx), expire))
			}) as ConnFuture<Expire<Http1Send<B>>>
		}))
	})
}

pub(super) fn http2<S, B>(
	expire: ExpireConfig,
) -> impl Layer<S, Service = BoxCloneService<(), Expire<Http2Send<B>>, BoxError>> + Clone
where
	S: Service<(), Response = super::super::HttpIo, Error = BoxError> + Clone + Send + 'static,
	S::Future: Send + 'static,
	B: hyper::body::Body + Send + Unpin + 'static,
	B::Data: Send,
	B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
	tower::layer::layer_fn(move |connector: S| {
		BoxCloneService::new(service_fn(move |_: ()| {
			let connector = connector.clone();
			Box::pin(async move {
				let io = connector.oneshot(()).await?;
				debug_assert!(io.is_negotiated_h2());

				console_log!("epoxy-client: using h2 pooled service");
				let (tx, conn) = conn::http2::Builder::new(WasmExecutor)
					.handshake(io)
					.await
					.map_err(|err| -> BoxError { Box::new(err) })?;
				WasmExecutor.execute(async move {
					let _ = conn.await;
				});

				Ok(Expire::new(Http2Send(tx), expire))
			}) as ConnFuture<Expire<Http2Send<B>>>
		}))
	})
}

impl<B> Service<Request<B>> for Http1Send<B>
where
	B: hyper::body::Body + Send + Unpin + 'static,
	B::Data: Send,
	B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
	type Response = Response<Incoming>;
	type Error = BoxError;
	type Future = ConnFuture<Self::Response>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.0
			.poll_ready(cx)
			.map_err(|err| -> BoxError { Box::new(err) })
	}

	fn call(&mut self, req: Request<B>) -> Self::Future {
		let fut = self.0.send_request(req);
		Box::pin(async move { fut.await.map_err(|err| -> BoxError { Box::new(err) }) })
	}
}

impl<B> Service<Request<B>> for Http2Send<B>
where
	B: hyper::body::Body + Send + Unpin + 'static,
	B::Data: Send,
	B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
	type Response = Response<Incoming>;
	type Error = BoxError;
	type Future = ConnFuture<Self::Response>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.0
			.poll_ready(cx)
			.map_err(|err| -> BoxError { Box::new(err) })
	}

	fn call(&mut self, req: Request<B>) -> Self::Future {
		let fut = self.0.send_request(req);
		Box::pin(async move { fut.await.map_err(|err| -> BoxError { Box::new(err) }) })
	}
}
