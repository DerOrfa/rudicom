use crate::tools::{Context, Result};
use std::io::{Error, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::Poll;
use tokio::fs::File;

pub struct AsyncMd5(md5::Context);

impl AsyncMd5
{
	pub fn new() -> Self
	{ Self(md5::Context::new()) }
	pub fn finalize(self) -> md5::Digest { self.0.finalize()}
}
impl tokio::io::AsyncWrite for AsyncMd5{
	fn poll_write(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>, buf: &[u8]) -> Poll<std::result::Result<usize, Error>> {
		Poll::Ready(self.get_mut().0.write(buf))
	}

	fn poll_flush(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<std::result::Result<(), Error>> {
		Poll::Ready(self.get_mut().0.flush())
	}

	fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<std::result::Result<(), Error>> {
		Poll::Ready(Ok(()))
	}
}

pub async fn compute_md5(filename:&Path) -> Result<md5::Digest>
{
	let mut md5_compute = AsyncMd5::new();
	let mut fileob = File::open(&filename).await.context(format!("opening {}",filename.display()))?;
	tokio::io::copy(&mut fileob,&mut md5_compute).await?;
	Ok(md5_compute.finalize())

}
