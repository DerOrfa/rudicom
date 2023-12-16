use std::collections::BTreeMap;
use anyhow::{Context, Result};
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
use crate::dcm::gen_filepath;
use crate::storage::async_store::write_file;
use crate::tools::complete_filepath;
use crate::db;
use crate::db::RegistryGuard;

pub(crate) async fn store(obj:DefaultDicomObject,checksum:md5::Digest) -> Result<Option<db::Entry>>
{
	let path = gen_filepath(&obj)?;
	let fileinfo:BTreeMap<String,sql::Value>= BTreeMap::from([
		("path".into(),path.clone().into()),
		("owned".into(),true.into()),
		("md5".into(),format!("{:x}", checksum).into())
	]);

	let mut guard= RegistryGuard::default();
	let registered = db::register_instance(&obj,vec![("file".into(),fileinfo.into())],Some(&mut guard)).await?;
	if registered.is_none() { //no previous data => normal register => store the file
		let path = complete_filepath(&path);
		let p = path.parent().unwrap();
		tokio::fs::create_dir_all(p).await.context(format!("Failed creating storage path {:?}",p))?;
		write_file(&path,&obj,None).await.context(format!("Failed to write file {}",path.display()))?;
		guard.reset();//all good, file stored, we can drop the guard
	}
	Ok(registered)
}
