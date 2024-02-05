use std::path::PathBuf;
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

	let fileinfo = db::File::from_owned(path,checksum.compute());
	assert_eq!(c_path, fileinfo.get_path());

	let registered = db::register_instance(&obj,vec![
		("file".into(),sql::Object::try_from(fileinfo).unwrap().into())
	],Some(&mut guard)).await?;
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

pub(crate) async fn import<T>(filename:T) -> crate::tools::Result<Option<db::Entry>> where T:Into<PathBuf>
{
	let mut checksum = md5::Context::new();
	let filename:PathBuf = filename.into();
	let context = format!("creating file info for {}",filename.to_string_lossy());
	let obj = read_file(&filename,Some(&mut checksum)).await?;
	let fileinfo = db::File::from_unowned(filename,checksum.compute()).context(context)?;

	db::register_instance(&obj,vec![
		("file".into(),sql::Object::try_from(fileinfo).unwrap().into())
	],None).await
}
