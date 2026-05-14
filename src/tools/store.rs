use std::io::ErrorKind;
use crate::db::{lookup, RecordId, RegisterResult, RegistryGuard};
use crate::tools::{Context, Error};
use crate::{db, tools};
use dicom::object::DefaultDicomObject;
use std::path::{Path, PathBuf};
use dicom::dictionary_std::tags;
use surrealdb::types as db_types;
use crate::db::RegisterResult::AlreadyStored;
use crate::storage::async_store;
use crate::tools::Error::{DataConflict, DicomError};

/// check if a path is a subdirectory of the storage path defined in config
pub fn is_storage<T:AsRef<Path>>(path:T) -> bool
{
	path.as_ref().starts_with(&crate::config::get().paths.storage_path)
}

struct FileGuard(Option<db::File>);

impl FileGuard {
	fn new(file: db::File) -> Self {Self(Some(file))}
	fn release(&mut self) -> Option<db::File> { self.0.take() }
	fn try_as_value(&self) -> tools::Result<db_types::Value>{
		db_types::Value::try_from(self.0.clone().unwrap())
	}
}

impl Drop for FileGuard {
	fn drop(&mut self) {
		if let Some(file) = self.0.take() {
			tokio::spawn(file.remove());
		}
	}
}

/// Stores given a dicom object as a file and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store(obj:DefaultDicomObject) -> tools::Result<RegisterResult>
{
	let uid = obj.element(tags::SOP_INSTANCE_UID)
		.map_err(|e|DicomError(e.into())).map(|e|e.to_str().unwrap().to_string())?;

	let (obj, mut fileinfo) = match db::File::new_from_obj(obj).await {
		Ok((obj,fileinfo)) => Ok((obj,FileGuard::new(fileinfo))),
		Err(Error::FileIOError {inner,path}) => {
			if let ErrorKind::AlreadyExists = inner.kind()
			{ // file is already there, lets see, if that's what we have
				let rec = RecordId::from_instance(uid.clone());
				let ctx = format!("looking for entry of existing file {}",path.display());
				let db_entry = lookup(&rec).await.transpose()
					.unwrap_or(Err(Error::MissingEntry(rec.clone())))
					.context(ctx.clone())?;
				let in_db = db_entry.get_file().context(ctx)?
					.read().await?;
				let incoming = async_store::read(&path).await
					.context(format!("reading existing file {} for comparison with {rec}",path.display()))?;
				return if in_db == incoming {
					Ok(AlreadyStored(rec))
				} else {
					Err(DataConflict(db_entry))
				}
			}
			Err(inner.into())
		},
		Err(e) => Err(e),
	}?;
	let mut guard= RegistryGuard::default();

	match db::register_instance(&obj, vec![
		("file",fileinfo.try_as_value()?)
	], &mut guard).await{
		// normal register => file is stored => confirm to caller
		Ok(RegisterResult::Stored(s)) => { //the file is stored, we can commit
			guard.commit().await?;
			fileinfo.release();
			Ok(RegisterResult::Stored(s))
		},
		// stuff's already there, apparently in another file, just leave it
		Ok(r) => {
			guard.reset().await?;
			Ok(r)
		}
		// failed to register, remove the file we wrote
		Err(e) => {
			guard.reset().await?;
			Err(e)
		}
	}
}

/// Stores given dicom file as file (makes a copy) and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store_file(filename:PathBuf) -> tools::Result<RegisterResult>
{
	store(async_store::read(&filename).await?).await
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
	let add_meta = vec![("file", db_types::Value::try_from(fileinfo)?.into())];
	let registered=db::register_instance(&obj,add_meta, &mut Default::default()).await;
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
pub async fn move_file(filename: &Path) -> tools::Result<RegisterResult>
{
	if is_storage(filename) { // if the file is already in the storage-path just import it, and take ownership
		import_file_impl(filename,true).await
	} else { // if not, store (aka copy) file and delete the source once we're done
		let stored= store_file(filename.to_owned()).await?;
		if let RegisterResult::Stored(_) = stored { //no error and no previously existing file, we can delete the source
			tokio::fs::remove_file(filename).await.context(format!("moving file {:?}",filename))?;
		}
		Ok(stored)
	}
}
