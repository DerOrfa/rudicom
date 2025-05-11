use crate::db::{lookup, RegisterResult, RegistryGuard};
use crate::dcm::gen_filepath;
use crate::storage::async_store::{read, write};
use crate::tools::Context;
use crate::{db, tools};
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
pub fn is_storage<T:AsRef<Path>>(path:T) -> bool
{
	path.as_ref().starts_with(&crate::config::get().paths.storage_path)
}


/// Stores given a dicom object as a file and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store(obj:DefaultDicomObject) -> tools::Result<RegisterResult>
{
	let mut guard= RegistryGuard::default();
	let mut checksum = md5::Context::new();
	let buffer= write(&obj,Some(&mut checksum))?;

	let fileinfo = db::File::new(gen_filepath(&obj)?, checksum.compute(), true, buffer.len() as u64);
	let c_path = fileinfo.get_path();
	let fileinfo = surrealdb::Value::try_from(fileinfo)?;

	let registered= db::register_instance(&obj, vec![("file",fileinfo.into())],Some(&mut guard)).await?;
	if let RegisterResult::Stored(_) = &registered { // normal register => store the file
		let p = c_path.parent().unwrap();
		let lossy_cpath= c_path.display();
		tokio::fs::create_dir_all(p).await.context(format!("Failed creating storage path {:?}",p))?;
		let mut file = OpenOptions::new().write(true).create_new(true).open(c_path.as_path()).await
			.context(format!("creating file {lossy_cpath}"))?;
		file.write_all(buffer.as_slice()).await
			.context(format!("writing to file {lossy_cpath}"))?;
		file.flush().await.context(format!("closing file {lossy_cpath}"))?;
		guard.reset();//all good, the file is stored, we can drop the guard
	} 
	Ok(registered)
}

/// Stores given dicom file as file (makes a copy) and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store_file(filename:&Path) -> tools::Result<RegisterResult>
{
	let buffer = read_to_buffer(filename).await?;
	store(read(buffer)?).await
}

/// Registers an existing file without storing (data won't be changed).
///
/// If the data already exists, the store is aborted but considered successful if existing data are equal.
/// 
/// If the existing data has a different checksum, an error is returned
pub async fn import_file(filename:&Path) -> tools::Result<RegisterResult>
{
	import_file_impl(filename, is_storage(filename)).await	
}

async fn import_file_impl(path:&Path,own:bool) -> tools::Result<RegisterResult>
{
	let (fileinfo,obj) = db::File::new_from_existing(path,own).await?;
	let my_md5= fileinfo.get_md5().to_string();
	let add_meta = vec![("file", surrealdb::Value::try_from(fileinfo)?.into())];
	let registered=db::register_instance(&obj,add_meta,None).await?;
	if let RegisterResult::AlreadyStored(existing) = &registered //if register says equal data exist, we check md5sum
	{
		let existing_file = lookup(existing).await?
			.expect("existing entry should exist").get_file()?;
		let existing_md5 = existing_file.get_md5();
		if existing_md5 != my_md5 {
			return Err(tools::Error::Md5Conflict {
				existing_md5:existing_md5.to_string(),
				existing_id:existing.clone(),
				my_md5:my_md5.to_string(),
			})
		} 
	}
	Ok(registered)
}

/// Registers an existing file without storing (data won't be changed) and moves the file to the storage path.
///
/// If the data already exists, the store is aborted but considered successful if existing data are equal.
///
/// If the existing data has a different checksum, an error is returned
pub async fn move_file(filename: &Path) -> tools::Result<RegisterResult>
{
	if is_storage(filename) { // if the file is already in the storage-path just import it, and take ownership
		import_file_impl(filename,true).await
	} else { // if not, store (aka copy) file and delete the source once we're done
		let stored= store_file(filename).await?;
		if let RegisterResult::Stored(_) = stored { //no error and no previously existing file, we can delete the source
			tokio::fs::remove_file(filename).await.context(format!("moving file {:?}",filename))?;
		}
		Ok(stored)
	}
}
