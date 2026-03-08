use super::Result;
use crate::db::Entry;
use crate::dcm::gen_filepath;
use async_tar::{Builder, Header};
use axum::body::Bytes;
use futures::{FutureExt, SinkExt, Stream, StreamExt};
use std::io::{ErrorKind, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::{CopyToBytes, SinkWriter};
use tokio_util::sync::PollSender;

pub struct TarStream
{
	inner:ReceiverStream<Bytes>,
	job:JoinHandle<Result<()>>
}

impl TarStream
{
	pub(crate) fn new(entry: Entry) -> TarStream
	{
		let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(1);
		let sink = PollSender::new(tx).sink_map_err(|_| ErrorKind::BrokenPipe);

		// Wrap it in `CopyToBytes` to get a `Sink<&[u8]>`.
		let writer = SinkWriter::new(CopyToBytes::new(sink));
		let inner = ReceiverStream::new(rx);
		let job = tokio::spawn(async {
			let mut r = make_tar(entry,writer).await?;
			r.flush().await.map_err(|e|e.into())
		});

		TarStream{inner,job}
	}
}

impl Stream for TarStream
{
	type Item = Result<Bytes>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = self.get_mut();

		match this.job.poll_unpin(cx) {
			// join error (task panicked or was canceled
			Poll::Ready(Err(e)) => {todo!()},
			// task returned an error
			Poll::Ready(Ok(Err(e))) => return Poll::Ready(Some(Err(e))),
			// were done here pipe has been flushed and closed
			Poll::Ready(Ok(Ok(_))) => return Poll::Ready(None),
			_ => {}
		}

		// task is still running extract data
		match this.inner.poll_next_unpin(cx)
		{
			Poll::Pending => Poll::Pending,
			Poll::Ready(d) => Poll::Ready(Ok(d).transpose()),
		}
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
		let meta = tokio::fs::metadata(file.get_path()).await?;
		let mut source = tokio::fs::File::open(file.get_path()).await?;
		sink.append_file(path,&mut source).await?;
	}
	let mut hd=Header::new_gnu();
	hd.set_mtime(std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
	hd.set_mode(0o644);
	sink.append_data(&mut hd,"md5sum", md5sum).await?;
	sink.into_inner().await.map_err(|e|e.into())
}