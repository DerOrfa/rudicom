use std::path::Path;
use dicom::object::{from_reader, DefaultDicomObject};
use md5::Digest;
use tokio::task::spawn_blocking;
use crate::storage::file::StandardFile;
use crate::tools;
use crate::tools::Context;
use crate::tools::error::DicomError;

pub mod async_store;
mod file;
pub mod image;

pub type Image<C = StandardFile> = image::Image<C>;

pub async fn checked_load(path:impl AsRef<Path>) -> tools::Result<(DefaultDicomObject,Digest)>{
	let path = path.as_ref();
	let reader = std::fs::File::open(path).context(format!("opening {}",path.display()))?;

	let obj_task= spawn_blocking(move||{
		let mut md5_context = md5::Context::new();
		let reader = crate::db::file::Md5Proxy {context:&mut md5_context,inner:reader};
		(from_reader(reader), md5_context)
	});

	obj_task.await.map_err(tools::Error::from)
		.and_then(|(r,md5)|
			r.map(|d|(d,md5.finalize()))
				.map_err(|e|DicomError::from(e).into())
		)
		.context(format!("reading {}", path.display()))		
} 