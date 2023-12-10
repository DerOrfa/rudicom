pub mod store;
pub mod remove;
pub mod import;

use std::path::{Path, PathBuf};
use anyhow::{anyhow, Context};
use dicom::object::DefaultDicomObject;
use surrealdb::sql::Thing;
pub use remove::remove;
use crate::{db, storage};

pub fn reduce_path(paths:Vec<PathBuf>) -> PathBuf
{
	let first=paths.first().expect("path list must not be empty");
	let mut last_pos=0;
	for base in first.ancestors()
	{
		if let Some(pos)=paths.iter().skip(last_pos).position(|p|!p.starts_with(base)){
			last_pos=pos;
		} else { return base.to_path_buf(); }
	}
	PathBuf::new()
}

pub fn complete_filepath<P>(path:&P) -> PathBuf where P:AsRef<Path>
{
	let root:PathBuf = crate::config::get("storage_path").expect(r#""storage_path" missing or invalid in config"#);
	root.join(path)
}
pub async fn get_instance_dicom(id:&str) -> anyhow::Result<Option<DefaultDicomObject>>
{
	if let Some(file)=lookup_instance_file(id).await.context("looking up fileinfo failed")?
	{
		let path = file.get_path();
		let checksum=file.md5.as_str();
		let mut md5=md5::Context::new();
		let obj=storage::async_store::read_file(path,Some(&mut md5)).await?;
		if format!("{:x}", md5.compute()) == checksum{Ok(Some(obj))}
		else {Err(anyhow!(r#"found checksum '{}' doesn't fit the data"#,checksum))}
	} else { Ok(None) }
}
pub(crate) async fn lookup_instance_file(id:&str) -> anyhow::Result<Option<db::File>>
{
	let id = Thing::from(("instances",id));
	if let Some(mut e)= db::lookup(&id).await.context(format!("failed looking for file in {}", id))?
	{
		let file:db::File = e.remove("file")
			.ok_or(anyhow!(r#""file" missing in entry instance:{}"#,id))?
			.try_into()?;
		Ok(Some(file))
	} else {Ok(None)}
}

pub async fn lookup_instance_filepath(id:&str) -> anyhow::Result<Option<PathBuf>>
{
	lookup_instance_file(id).await.context("looking up fileinfo failed")
		.map(|f|f.map(|f|f.get_path()))
}
