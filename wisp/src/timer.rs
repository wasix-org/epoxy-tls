use std::{future::Future, time::Duration};

pub trait Timer {
	fn sleep(&mut self, duration: Duration) -> impl Future<Output = ()> + Send;
}

pub(crate) struct NoTimer;
impl Timer for NoTimer {
	#[allow(clippy::unused_async_trait_impl)]
	async fn sleep(&mut self, _duration: Duration) {
		panic!("timer not implemented")
	}
}

#[cfg(feature = "tokio")]
mod tokio_timer {
	use super::Timer;
	use std::time::Duration;
	use tokio::time::sleep;

	pub struct TokioTimer;
	impl Timer for TokioTimer {
		async fn sleep(&mut self, duration: Duration) {
			sleep(duration).await;
		}
	}
}
#[cfg(feature = "tokio")]
pub use tokio_timer::TokioTimer;
