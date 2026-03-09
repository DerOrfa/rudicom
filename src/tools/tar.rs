use super::Result;
use crate::db::Entry;
use crate::dcm::gen_filepath;
use async_tar::{Builder, Header};
use axum::body::Bytes;
use futures::{FutureExt, Stream};
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;

pub struct TarStream
{
	inner:ReaderStream<DuplexStream>,
	job:JoinHandle<Result<()>>
}

impl TarStream
{
	pub(crate) fn new<F,FT,W>(entry: Entry, f:F) -> TarStream
		where F:FnOnce(Entry,DuplexStream) -> FT + Send + 'static,
			  FT:Future<Output=Result<W>>  + Send,
			  W:AsyncWrite + Unpin + Send + Sync
	{
		let (tx,rx) = tokio::io::duplex(1024*1024*10);

		let job = tokio::spawn(async {
			let mut r = f(entry,tx).await?;
			r.flush().await.map_err(|e|e.into())
		});

		TarStream{inner:ReaderStream::new(rx),job}
	}
}

impl Stream for TarStream
{
	type Item = Result<Bytes>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = self.get_mut();

		if !this.job.is_finished(){
		match this.job.poll_unpin(cx) {
			// join error (task panicked or was canceled)
			Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e.into()))),
			// task returned an error
			Poll::Ready(Ok(Err(e))) => return Poll::Ready(Some(Err(e))),
			_ => {}
		}}

		// task is still running extract data
		Pin::new(&mut this.inner).poll_next(cx).map_err(|e|e.into())
	}
}

/// Create a tar from an `Entry`
///
/// Creates a tar from all instances in an `Entry` and writes it into `sink`.
/// Filenames inside are generated from `filename_pattern` inside the config regardless if they are owned or not.
/// All metadata are taken directly from the stored files.
/// Adds a md5sum file containing the checksums.
pub(crate) async fn make_tar<W:AsyncWrite + Unpin + Send + Sync>(entry: Entry, sink:W) -> Result<W>
{
	let mut sink = Builder::new(sink);
	let files = entry.get_files().await?;
	let mut md5sum = std::io::Cursor::new(vec![]);
	for file in files
	{
		let path = gen_filepath(&file.read().await?)?;
		writeln!(&mut md5sum, "{} {}", file.get_md5(), path)?;
		let mut source = tokio::fs::File::open(file.get_path()).await?;
		sink.append_file(path,&mut source).await?;
	}
	let mut hd=Header::new_gnu();
	hd.set_mtime(std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
	hd.set_mode(0o644);
	sink.append_data(&mut hd,"md5sum", md5sum).await?;
	sink.into_inner().await.map_err(|e|e.into())
}