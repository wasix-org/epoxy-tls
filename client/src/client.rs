use hyper::{Request, Response, body::Incoming};
use tower::Service;

use crate::provider::http::{HyperClient, HyperClientBody};

pub struct Client {
	hyper: HyperClient,
}

impl Client {
	fn build_client(
		&self,
	) -> impl Service<Request<HyperClientBody>, Response = Response<Incoming>> {
		self.hyper.clone()
	}
}
