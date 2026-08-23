use crate::db::RegisterResult;
use crate::db::register_manager::RegisterManager;
use crate::storage::Image;
use crate::{config, storage, tools};
use dicom::object::DefaultDicomObject;
use std::io::ErrorKind;
use std::path::Path;

/// check if a path is a subdirectory of the storage path defined in config
pub fn is_storage<T: AsRef<Path>>(path: T) -> bool
{
	path.as_ref().starts_with(&config::get().paths.storage_path)
}

pub async fn single_register(image:storage::Image)	-> tools::Result<RegisterResult>
{
	let mut session =RegisterManager::new();
	let store = session.register(image).await?;
	session.flush().await;
	match store.await {
		Ok(r) => r,
		Err(e) => Err(tools::Error::IoError(std::io::Error::new(ErrorKind::BrokenPipe,e)))
	}
}


/// Stores a single dicom object as a file and registers it as owned (might change data).
/// 
/// If the object already exists, the store is aborted but considered successful if existing data are equal.
pub async fn store_single_ob(obj:DefaultDicomObject) -> tools::Result<RegisterResult>
{
	single_register(Image::from_obj_filtered(obj)?).await
}

/// Registers a single existing file without storing (data won't be changed).
///
/// If the data already exists, the store is aborted but considered successful if existing data are equal.
/// 
/// If the existing data has a different checksum, an error is returned
pub async fn import_single_file(path:&Path) -> tools::Result<RegisterResult>
{
	single_register(storage::Image::from_existing(path).await?).await
}

/// Registers an existing file and moves the file to the storage path (data won't be changed).
///
/// If the data already exists, the store is aborted but considered successful if existing data are equal.
///
/// If the existing data has a different checksum, an error is returned
pub async fn move_single_file(path:&Path) -> tools::Result<RegisterResult>
{
	single_register(storage::Image::move_existing(path).await?).await
}
