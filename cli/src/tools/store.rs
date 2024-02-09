use std::path::Path;
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::dcm::gen_filepath;
use crate::storage::async_store::{read, write};
use crate::tools::complete_filepath;
use crate::db;
use crate::db::RegistryGuard;
use crate::tools::Context;

/// stores given dicom object as file and registers it as owned (might change data)
/// if the object already exists, the store is aborted and the existing data is returned
/// None otherwise
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

/// stores given dicom file as file (makes a copy) and registers it as owned (might change data)
/// if the object already exists, the store is aborted and the existing data is returned
/// None otherwise
pub(crate) async fn store_file<'a,T>(filename:T) -> crate::tools::Result<Option<db::Entry>> where T:Into<&'a Path>
{
	let mut buffer = Vec::<u8>::new();
	File::open(filename.into()).await?.read_to_end(&mut buffer).await?;
	store(read(buffer)?).await
}

/// registers an existing file without storing (data won't be changed)
/// there is a chance the file is already registered if that's the case its information is returned
/// as usual and no registration takes place.
/// Additionally, if the existing data has a different md5, the new md5 is added as
/// "conflicting_md5" to the returned data
pub(crate) async fn import<'a,T>(filename:T) -> crate::tools::Result<Option<db::Entry>> where T:Into<&'a Path>
{
	let filename:&Path = filename.into();
	let mut buffer = Vec::<u8>::new();
	File::open(filename).await?.read_to_end(&mut buffer).await?;
	let checksum= md5::compute(buffer);
	let fileinfo = db::File::from_unowned(filename,checksum)
		.context(format!("creating file info for {}",filename.to_string_lossy()))?;
	let obj= fileinfo.read().await?;
	let mut reg=db::register_instance(&obj,vec![
		("file",sql::Object::try_from(fileinfo)?.into())
	],None).await;
	if let Ok(Some(existing)) = &mut reg
	{
		let my_md5 = format!("{:x}",checksum);
		if existing.get_file().unwrap().get_md5() != my_md5.as_str(){
			existing.insert("conflicting_md5",my_md5);
		}
	}
	reg
}
