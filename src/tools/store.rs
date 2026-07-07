use crate::db::RegisterResult::AlreadyStored;
use crate::db::{lookup, File, FileInfo, RegisterResult, Session};
use crate::storage::async_store;
use crate::tools::{Context, Error};
use crate::{db, tools};
use dicom::object::DefaultDicomObject;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use surrealdb::engine::any::Any;

/// check if a path is a subdirectory of the storage path defined in config
pub fn is_storage<T:AsRef<Path>>(path:T) -> bool
{
	path.as_ref().starts_with(&crate::config::get().paths.storage_path)
}

/// Stores a dicom object as a file and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store_ob<S>(obj:DefaultDicomObject, session: &mut S) -> tools::Result<RegisterResult> where S:Session<Any>
{
	db::register_instance(Arc::new(obj), &mut FileInfo::Store, session).await
}

/// Stores given dicom file as file (makes a copy) and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store_file<S>(filename:PathBuf, session: &mut S) -> tools::Result<RegisterResult> where S:Session<Any>
{
	store_ob(async_store::read(&filename).await?, session).await
}

/// Registers an existing file without storing (data won't be changed).
///
/// If the data already exists, the store is aborted but considered successful if existing data are equal.
/// 
/// If the existing data has a different checksum, an error is returned
pub async fn import_file<S>(path:&Path, session: &mut S) -> tools::Result<RegisterResult> where S: Session<Any>
{
	let (info,obj) = db::File::new_from_existing(path,is_storage(path)).await?;
	import_file_ob(info, obj, session).await
}

pub async fn import_file_ob<S>(info:File,obj:DefaultDicomObject, session: &mut S) -> tools::Result<RegisterResult> where S:Session<Any>
{
	let my_md5= info.get_md5().to_string();
	let registered=db::register_instance(Arc::new(obj),&mut FileInfo::Exists(info), session).await;
	let registered = registered?;
	if let AlreadyStored(existing) = &registered //if register says equal data exist, we check md5sum
	{
		let existing_file = lookup(existing).await?
			.expect("existing entry should exist").get_file()?;
		let existing_md5 = existing_file.get_md5();
		if existing_md5 != my_md5 {
			return Err(Error::Md5Conflict {
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
pub async fn move_file_ob<S>(info: File, obj: DefaultDicomObject, session: &mut S) -> tools::Result<RegisterResult> where S:Session<Any>
{
	if info.owned { // if the file is already owned just import it
		import_file_ob(info,obj, session).await
	} else { // if not, store (aka copy) file and delete the source once we're done
		let stored = store_ob(obj, session).await?;
		if let RegisterResult::Stored(_) = stored { //no error and no previously existing file, we can delete the source
			tokio::fs::remove_file(info.get_path()).await.context(format!("moving file {}", info.get_path().display()))?;
		}
		Ok(stored)
	}
}
