use std::collections::BTreeMap;
use std::io::{Error, Write};
use std::pin::Pin;
use std::task::Poll;
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use crate::dcm::gen_filepath;
use crate::storage::async_store::write;
use crate::tools::complete_filepath;
use crate::db;
use crate::db::RegistryGuard;
use crate::tools::Context;

pub(crate) struct AsyncMd5(md5::Context);

impl AsyncMd5
{
	pub fn new() -> Self
	{Self(md5::Context::new())}
	pub fn compute(self) -> String
	{format!("{:x}", self.0.compute())}
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

pub(crate) async fn store(obj:DefaultDicomObject) -> crate::tools::Result<Option<db::Entry>>
{
	let path = gen_filepath(&obj)?;

	let mut guard= RegistryGuard::default();
	let mut checksum = md5::Context::new();
	let buffer=write(&obj,Some(&mut checksum))?.into_inner();

	let fileinfo:BTreeMap<String,sql::Value>= BTreeMap::from([
		("path".into(),path.clone().into()),
		("owned".into(),true.into()),
		("md5".into(),format!("{:x}", checksum.compute()).into())
	]);

	let registered = db::register_instance(&obj,vec![("file".into(),fileinfo.into())],Some(&mut guard)).await?;
	if registered.is_none() { //no previous data => normal register => store the file
		let path = complete_filepath(&path);
		let p = path.parent().unwrap();
		tokio::fs::create_dir_all(p).await.context(format!("Failed creating storage path {:?}",p))?;

		let mut file = OpenOptions::new().create_new(true).open(path).await?;;
		file.write_all(buffer.as_slice()).await?;
		file.flush().await?;

		guard.reset();//all good, file stored, we can drop the guard
	}
	Ok(registered)
}

