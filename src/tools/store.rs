use crate::db;
use crate::db::RegistryGuard;
use crate::dcm::gen_filepath;
use crate::storage::async_store::{read, write};
use crate::tools::Context;
use dicom::object::DefaultDicomObject;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn read_to_buffer(filename:&Path) -> crate::tools::Result<Vec<u8>>
{
	let mut buffer = Vec::<u8>::new();
	let mut file = File::open(filename).await?;
	file.read_to_end(&mut buffer).await?;
	Ok(buffer)
}

/// check if a path is a subdirectory of the storage path defined in config
pub(crate) fn is_storage<T:AsRef<Path>>(path:T) -> bool
{
	path.as_ref().starts_with(&crate::config::get().paths.storage_path)
}


/// stores given dicom object as file and registers it as owned (might change data)
/// if the object already exists, the store is aborted and the existing data is returned
/// None otherwise
pub(crate) async fn store(obj:DefaultDicomObject) -> crate::tools::Result<Option<db::Entry>>
{
	let mut guard= RegistryGuard::default();
	let mut checksum = md5::Context::new();
	let buffer= write(&obj,Some(&mut checksum))?;

	let fileinfo = db::File::new(gen_filepath(&obj)?, checksum.compute(), true, buffer.len() as u64);
	let c_path = fileinfo.get_path();
	let fileinfo = surrealdb::Value::try_from(fileinfo)?;

	let registered = db::register_instance(&obj,
	   vec![("file",fileinfo.into())],
		Some(&mut guard)
	).await?;
	if registered.is_none() { //no previous data => normal register => store the file
		let p = c_path.parent().unwrap();
		let lossy_cpath= c_path.display();
		tokio::fs::create_dir_all(p).await.context(format!("Failed creating storage path {:?}",p))?;
		let mut file = OpenOptions::new().write(true).create_new(true).open(c_path.as_path()).await
			.context(format!("creating file {lossy_cpath}"))?;
		file.write_all(buffer.as_slice()).await
			.context(format!("writing to file {lossy_cpath}"))?;
		file.flush().await?;

		guard.reset();//all good, file stored, we can drop the guard
	}
	Ok(registered)
}

/// stores given dicom file as file (makes a copy) and registers it as owned (might change data)
/// if the object already exists, the store is aborted and the existing data is returned
/// None otherwise
pub(crate) async fn store_file(filename:&Path) -> crate::tools::Result<Option<db::Entry>>
{
	let buffer = read_to_buffer(filename).await?;
	store(read(buffer)?).await
}

/// registers an existing file without storing (data won't be changed)
/// there is a chance the file is already registered if that's the case its information is returned
/// as usual and no registration takes place.
/// Additionally, if the existing data has a different md5, the new md5 is added as
/// "conflicting_md5" to the returned data
pub(crate) async fn import_file(filename:&Path) -> crate::tools::Result<Option<db::Entry>>
{
	import_file_impl(filename, false).await	
}

async fn import_file_impl(path:&Path,own:bool) -> crate::tools::Result<Option<db::Entry>>
{
	let (fileinfo,obj) = db::File::new_from_existing(path,own).await?;
	let my_md5= fileinfo.get_md5().to_string();
	let mut reg=db::register_instance(&obj,vec![
		("file", surrealdb::Value::try_from(fileinfo)?.into())
	],None).await;
	if let Ok(Some(existing)) = &mut reg
	{
		if existing.get_file()?.get_md5() != my_md5 {
			existing.insert("conflicting_md5",my_md5);
		}
	}
	reg
}

pub(crate) async fn move_file(filename: &Path) -> crate::tools::Result<Option<db::Entry>> 
{
	if is_storage(filename) { // if the file is already in the storage-path just import it, and take ownership
		import_file_impl(filename,true).await
	} else { // if not, store (aka copy) file and delete source once we're done
		let existing= store_file(filename).await?;
		if existing.is_none(){ //no error, and no previously existing file, we can delete the source
			tokio::fs::remove_file(filename).await?;
		}
		Ok(existing)
	}
}
