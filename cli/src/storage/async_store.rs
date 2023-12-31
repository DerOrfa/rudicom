use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::Path;
use dicom::object::{DefaultDicomObject, from_reader};
use anyhow::{bail, Result};
use md5::Context;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt,AsyncReadExt};

pub fn read<T>(input: T, with_md5:Option<&mut Context>) -> Result<DefaultDicomObject> where T:AsRef<[u8]>
{
	let mut buffer = Cursor::new(input);
	if let Some( md5) = with_md5 {
		std::io::copy(&mut buffer,md5).unwrap();
		buffer.seek(SeekFrom::Start(0))?;
	}
	Ok(from_reader(buffer)?)
}

pub fn write(obj:&DefaultDicomObject, with_md5:Option<&mut Context>) -> Result<Cursor<Vec<u8>>>{
	let mut out = Cursor::new(Vec::new());
	out.seek(SeekFrom::Start(128))?;
	Write::write_all(&mut out, b"DICM")?;
	obj.write_meta(&mut out)?;
	obj.write_dataset(&mut out)?;
	if let Some( md5) = with_md5{
		out.seek(SeekFrom::Start(0))?;
		std::io::copy(&mut out,md5).unwrap();
	}
	out.seek(SeekFrom::Start(0))?;
	Ok(out)
}

pub async fn write_file<T>(path:T, obj:&DefaultDicomObject,with_md5:Option<&mut Context>)->Result<()> where T:AsRef<Path>
{
	let mut file = File::create(path).await?;
	let data= write(obj,with_md5)?.into_inner();
	file.write_all(data.as_slice()).await?;
	file.flush().await?;
	Ok(())
}

pub async fn read_file<T>(path:T,with_md5:Option<&mut Context>) -> Result<DefaultDicomObject> where T:AsRef<Path>
{
	let mut buffer = Vec::<u8>::new();
	File::open(path.as_ref()).await?.read_to_end(&mut buffer).await?;
	if buffer.len()==0 {bail!("There is no data in {}",path.as_ref().to_string_lossy())}
	read(buffer, with_md5)
}

