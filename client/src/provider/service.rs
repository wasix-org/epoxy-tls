use std::pin::Pin;

pub trait ProviderService<T> {
	type Response;
	type Error;
	type Future: Future<Output = Result<Self::Response, Self::Error>>;

	fn call(&self, request: T) -> Self::Future;
}

pub struct BoxProviderService<T, U, E> {
	inner: Box<
		dyn ProviderService<T, Response = U, Error = E, Future = BoxFuture<U, E>> + Send + Sync,
	>,
}

struct MapFuture<S, F> {
	inner: S,
	f: F,
}

impl<S, F> MapFuture<S, F> {
	fn new(inner: S, f: F) -> Self {
		Self { inner, f }
	}
}

impl<R, S, F, T, E, Fut> ProviderService<R> for MapFuture<S, F>
where
	S: ProviderService<R>,
	F: Fn(S::Future) -> Fut,
	E: From<S::Error>,
	Fut: Future<Output = Result<T, E>>,
{
	type Response = T;
	type Error = E;
	type Future = Fut;

	fn call(&self, req: R) -> Self::Future {
		(self.f)(self.inner.call(req))
	}
}

type BoxFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

impl<T, U, E> BoxProviderService<T, U, E> {
	#[allow(missing_docs)]
	pub fn new<S>(inner: S) -> Self
	where
		S: ProviderService<T, Response = U, Error = E> + Send + Sync + 'static,
		S::Future: Send + 'static,
	{
		// rust can't infer the type
		let inner: Box<
			dyn ProviderService<T, Response = U, Error = E, Future = BoxFuture<U, E>> + Send + Sync,
		> = Box::new(MapFuture::new(inner, |f: S::Future| Box::pin(f) as _));
		BoxProviderService { inner }
	}
}

impl<T, U, E> ProviderService<T> for BoxProviderService<T, U, E> {
	type Response = U;
	type Error = E;
	type Future = BoxFuture<U, E>;

	fn call(&self, request: T) -> BoxFuture<U, E> {
		self.inner.call(request)
	}
}
