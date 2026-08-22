use crate::db::register_manager::RegisterManager;
use crate::db::RegisterResult;
use crate::{config, storage, tools};
use dicom::object::DefaultDicomObject;
use pyo3::Python;
use pyo3::prelude::PyModule;
use std::ffi::CString;
use std::io::ErrorKind;
use std::path::Path;
use tokio::sync::oneshot;

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
	let mut session = RegisterManager::new();
	let store = store_ob(obj,&mut session).await?;
	session.flush().await;
	match store.await{
		Ok(r) => r,
		Err(e) => Err(tools::Error::IoError(std::io::Error::new(ErrorKind::BrokenPipe,e)))
	}
}

pub async fn store_ob(mut obj:DefaultDicomObject, session: &mut RegisterManager)
	-> tools::Result<oneshot::Receiver<tools::Result<RegisterResult>>>
{
	if !config::get().filters.is_empty(){
		Python::attach::<_,tools::Result<()>>(|py| {
			for (name,code) in config::get().filters.iter()
				.filter(|(_,code)| !code.is_empty())
			{
				let code = CString::new(code.as_str()).unwrap();
				let name = CString::new(name.as_str()).unwrap();
				let code = PyModule::from_code(py, code.as_ref(), c"", name.as_ref())?;
				tools::filter::filter(code, &mut obj)?;
			}
			Ok(())
		})?;
	}
	let image = storage::Image::from_obj(obj);
	session.register(image).await
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
