pub mod wisp;
//pub mod wsproxy;

pub use wisp::handle_wisp;
//pub use wsproxy::handle_wsproxy;
pub async fn handle_wsproxy(
	mut ws: crate::stream::WebSocketStreamWrapper,
	id: String,
	path: String,
	udp: bool,
) -> anyhow::Result<()> {
	todo!();
}
