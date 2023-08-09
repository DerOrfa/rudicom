use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::PathBuf;
use dicom::object::{DefaultDicomObject, from_reader};
use anyhow::Result;
use md5::Context;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt,AsyncReadExt};


fn mem_write(obj:&DefaultDicomObject) -> Result<Cursor<Vec<u8>>>{
	let mut out = Cursor::new(Vec::<u8>::new());
	out.seek(SeekFrom::Start(128))?;
	Write::write_all(&mut out, b"DICM")?;
	obj.write_meta(&mut out)?;
	obj.write_dataset(&mut out)?;
	Ok(out)
}

pub async fn write_file(path:PathBuf, obj:&DefaultDicomObject)->Result<()>{
	let mut file = File::create(path).await?;
	let data= mem_write(obj)?.into_inner();
	file.write_all(data.as_slice()).await?;
	file.flush().await?;
	Ok(())
}

pub async fn read_file(path:PathBuf,with_md5:Option<&mut Context>) -> Result<DefaultDicomObject>{
	let mut buffer = Vec::<u8>::new();
	File::open(path).await?.read_to_end(&mut buffer).await?;
	if let Some( md5) = with_md5{
		md5.write_all(buffer.as_slice()).unwrap();
	}
	buffer.drain(..128); // preamble
	Ok(from_reader(Cursor::new(buffer))?)
}
