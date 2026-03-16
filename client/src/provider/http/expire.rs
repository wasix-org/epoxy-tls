use std::{
	future::Future,
	pin::Pin,
	sync::{Arc, Mutex},
	task::{Context, Poll},
	time::Duration,
};

use instant::Instant;
use pin_project::pin_project;
use tower::Service;

use super::BoxError;

#[derive(Debug)]
pub(super) struct ConnectionExpired;

impl std::fmt::Display for ConnectionExpired {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("connection expired")
	}
}

impl std::error::Error for ConnectionExpired {}

#[derive(Clone, Copy, Debug)]
pub struct ExpireConfig {
	pub idle_timeout: Duration,
	pub max_lifetime: Duration,
}

#[derive(Clone, Debug)]
pub struct Expire<S> {
	inner: S,
	state: Arc<Mutex<ExpireState>>,
	config: ExpireConfig,
}

#[derive(Debug)]
struct ExpireState {
	created_at: Instant,
	last_used_at: Instant,
}

impl<S> Expire<S> {
	pub fn new(inner: S, config: ExpireConfig) -> Self {
		let now = Instant::now();
		Self {
			inner,
			state: Arc::new(Mutex::new(ExpireState {
				created_at: now,
				last_used_at: now,
			})),
			config,
		}
	}

	pub fn is_expired(&self) -> bool {
		let state = self.state.lock().unwrap();
		let now = Instant::now();
		now.duration_since(state.last_used_at) >= self.config.idle_timeout
			|| now.duration_since(state.created_at) >= self.config.max_lifetime
	}

	fn touch(&self) {
		self.state.lock().unwrap().last_used_at = Instant::now();
	}
}

#[pin_project]
pub struct ExpireFuture<F> {
	#[pin]
	inner: F,
}

impl<S, Req> Service<Req> for Expire<S>
where
	S: Service<Req, Error = BoxError>,
{
	type Response = S::Response;
	type Error = BoxError;
	type Future = ExpireFuture<S::Future>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		if self.is_expired() {
			return Poll::Ready(Err(Box::new(ConnectionExpired)));
		}

		match self.inner.poll_ready(cx) {
			Poll::Ready(Ok(())) => {
				self.touch();
				Poll::Ready(Ok(()))
			}
			Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
			Poll::Pending => Poll::Pending,
		}
	}

	fn call(&mut self, req: Req) -> Self::Future {
		self.touch();
		ExpireFuture {
			inner: self.inner.call(req),
		}
	}
}

impl<F, Res> Future for ExpireFuture<F>
where
	F: Future<Output = Result<Res, BoxError>>,
{
	type Output = Result<Res, BoxError>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.project().inner.poll(cx)
	}
}
