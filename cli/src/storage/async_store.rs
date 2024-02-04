use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::Path;
use dicom::object::{DefaultDicomObject, from_reader};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use crate::tools::Error::DicomError;
use crate::tools::Result;

pub fn read<T>(input: T, with_md5:Option<&mut md5::Context>) -> Result<DefaultDicomObject> where T:AsRef<[u8]>
{
	let mut buffer = Cursor::new(input);
	if let Some( md5) = with_md5 {
		std::io::copy(&mut buffer,md5).unwrap();
		buffer.seek(SeekFrom::Start(0))?;
	}
	from_reader(buffer).map_err(|e|DicomError(e.into()))
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

pub async fn read_file<T>(path:T,with_md5:Option<&mut md5::Context>) -> Result<DefaultDicomObject> where T:AsRef<Path>
{
	let mut buffer = Vec::<u8>::new();
	File::open(path.as_ref()).await?.read_to_end(&mut buffer).await?;
	if buffer.len()==0 {return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into())}
	read(buffer, with_md5)
}
