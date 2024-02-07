use std::path::Path;
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use crate::dcm::gen_filepath;
use crate::storage::async_store::{read_file, write};
use crate::tools::complete_filepath;
use crate::db;
use crate::db::RegistryGuard;
use crate::tools::Context;

/// stores given dicom object as file and registers it as owned (might change data)
pub(crate) async fn store(obj:DefaultDicomObject) -> crate::tools::Result<Option<db::Entry>>
{
	let path = gen_filepath(&obj)?;
	let c_path = complete_filepath(&path);

	let mut guard= RegistryGuard::default();
	let mut checksum = md5::Context::new();
	let buffer=write(&obj,Some(&mut checksum))?.into_inner();

	let fileinfo:sql::Object = db::File::from_owned(path,checksum.compute()).try_into()?;

	let registered = db::register_instance(&obj,
	   vec![("file",fileinfo.into())],
		Some(&mut guard)
	).await?;
	if registered.is_none() { //no previous data => normal register => store the file
		let p = c_path.parent().unwrap();
		tokio::fs::create_dir_all(p).await.context(format!("Failed creating storage path {:?}",p))?;
		let mut file = OpenOptions::new().create_new(true).open(c_path).await?;
		file.write_all(buffer.as_slice()).await?;
		file.flush().await?;

		guard.reset();//all good, file stored, we can drop the guard
	}
	Ok(registered)
}

/// registers an existing file without storing (data won't be changed)
pub(crate) async fn import<'a,T>(filename:T) -> crate::tools::Result<Option<db::Entry>> where T:Into<&'a Path>
{
	let mut checksum = md5::Context::new();
	let filename:&Path = filename.into();
	let context = format!("creating file info for {}",filename.to_string_lossy());
	let obj = read_file(filename,Some(&mut checksum)).await?;
	let fileinfo = db::File::from_unowned(filename,checksum.compute())
		.map_err(crate::tools::Error::from)
		.and_then(sql::Object::try_from)
		.context(context)?;

	db::register_instance(&obj,vec![("file",fileinfo.into())],None).await
}
