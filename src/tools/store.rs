use std::collections::BTreeMap;
use anyhow::{Context, Result};
use dicom::object::DefaultDicomObject;
use crate::{db, register_instance, RegistryGuard};
use crate::db::JsonValue;
use crate::dcm::gen_filepath;
use crate::file::async_store::write_file;

pub async fn store(obj:DefaultDicomObject,checksum:md5::Digest) -> Result<JsonValue>
{
	let path = gen_filepath(&obj);
	let fileinfo:BTreeMap<String,db::DbVal>= BTreeMap::from([
		("path".into(),path.to_str().unwrap().into()),
		("owned".into(),true.into()),
		("md5".into(),format!("{:x}", checksum).into())
	]);

	let mut guard=RegistryGuard::default();
	let registered = register_instance(&obj,vec![("file".into(),fileinfo.into())],Some(&mut guard)).await?;
	if registered.is_null() {
		write_file(&path,&obj).await.context(format!("Failed to write file {}",path.display()))?;
		guard.reset();//all good, we can drop the guard
	}
	Ok(registered)
}
