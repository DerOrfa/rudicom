use std::collections::BTreeMap;
use anyhow::{Context, Result};
use dicom::object::DefaultDicomObject;
use crate::{DbVal, register_instance, RegistryGuard};
use crate::JsonValue;
use crate::dcm::{complete_filepath, gen_filepath};
use crate::storage::async_store::write_file;

pub async fn store(obj:DefaultDicomObject,checksum:md5::Digest) -> Result<JsonValue>
{
	let path = gen_filepath(&obj);
	let fileinfo:BTreeMap<String,DbVal>= BTreeMap::from([
		("path".into(),path.clone().into()),
		("owned".into(),true.into()),
		("md5".into(),format!("{:x}", checksum).into())
	]);

	let mut guard=RegistryGuard::default();
	let registered = register_instance(&obj,vec![("file".into(),fileinfo.into())],Some(&mut guard)).await?;
	if registered.is_null() {
		let path = complete_filepath(path);
		write_file(&path,&obj,None).await.context(format!("Failed to write file {}",path.display()))?;
		guard.reset();//all good, we can drop the guard
	}
	Ok(registered)
}
