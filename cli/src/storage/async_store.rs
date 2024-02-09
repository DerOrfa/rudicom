use std::io::{Cursor, Error, Seek, SeekFrom, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::Poll;
use dicom::object::{DefaultDicomObject, from_reader};
use tokio::fs::File;
use crate::tools::Error::DicomError;
use crate::tools::{Context, Result};

pub(crate) struct AsyncMd5(md5::Context);

impl AsyncMd5
{
	pub fn new() -> Self
	{Self(md5::Context::new())}
	pub fn compute(self) -> md5::Digest	{self.0.compute()}
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

pub fn read<T>(input: T) -> Result<DefaultDicomObject> where T:AsRef<[u8]>
{
	from_reader(Cursor::new(input)).map_err(|e|DicomError(e.into()))
}

pub fn write(obj:&DefaultDicomObject, with_md5:Option<&mut md5::Context>) -> Result<Cursor<Vec<u8>>>{
	let mut out = Cursor::new(Vec::new());
	out.seek(SeekFrom::Start(128))?;
	Write::write_all(&mut out, b"DICM")?;
	obj
		.write_meta(&mut out)
		.and_then(|_|obj.write_dataset(&mut out))
		.map_err(|e|DicomError(e.into()))?;
	if let Some( md5) = with_md5{
		out.seek(SeekFrom::Start(0))?;
		std::io::copy(&mut out,md5).unwrap();
	}
	out.seek(SeekFrom::Start(0))?;
	Ok(out)
}

pub async fn compute_md5(filename:&Path) -> Result<md5::Digest>
{
	let mut md5_compute = AsyncMd5::new();
	let mut fileob = File::open(&filename).await.context(format!("opening {}",filename.to_string_lossy()))?;
	tokio::io::copy(&mut fileob,&mut md5_compute).await?;
	Ok(md5_compute.compute())

}
