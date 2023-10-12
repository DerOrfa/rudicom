pub mod store;
pub mod remove;
pub mod import;

use std::any::type_name;
use std::path::{Path, PathBuf};
use anyhow::{anyhow, Context};
use dicom::object::DefaultDicomObject;
use serde::de::DeserializeOwned;
use surrealdb::sql;
use surrealdb::sql::Thing;
pub use remove::remove;
pub use store::store;
use crate::{db, JsonMap, storage};

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

pub fn complete_filepath<P>(path:P) -> PathBuf where P:AsRef<Path>
{
	let root:PathBuf = crate::config::get("storage_path").expect(r#""storage_path" missing or invalid in config"#);
	root.join(path)
}
pub fn json_to_path(obj:&JsonMap) -> anyhow::Result<PathBuf>
{
	let owned:bool = obj
		.get("owned").ok_or(anyhow!(r#""owned" missing in file entry"#))?
		.as_bool().ok_or(anyhow!(r#""owned" should be bool"#))?;
	let path = obj
		.get("path").ok_or(anyhow!(r#""path" missing in file entry"#))?
		.as_str().ok_or(anyhow!(r#""path" should be bool"#))?;
	if owned{Ok(complete_filepath(path))}
	else {Ok(path.into())}
}

pub async fn get_instance_dicom(id:&str) -> anyhow::Result<Option<DefaultDicomObject>>
{
	if let Some(mut map)=lookup_instance_file(id).await.context("looking up fileinfo failed")?
	{
		let path:String = db::extract(&mut map,"path")?;
		let checksum:String=db::extract(&mut map, "md5")?;
		let path=match db::extract(&mut map,"owned")? {
			true => complete_filepath(path),
			false => PathBuf::from(path)
		};
		let mut md5=md5::Context::new();
		let obj=storage::async_store::read_file(path,Some(&mut md5)).await?;
		if format!("{:x}", md5.compute()) == checksum{Ok(Some(obj))}
		else {Err(anyhow!(r#"found checksum '{}' doesn't fit the data"#,checksum))}
	} else { Ok(None) }
}
pub async fn lookup_instance_file(id:&str) -> anyhow::Result<Option<sql::Object>>
{
	let id = Thing::from(("instances",id));
	if let Some(mut e)= crate::db::lookup(id.clone()).await.context(format!("failed looking for file in {}", id))?
	{
		match e.remove("file").ok_or(anyhow!(r#""file" missing in entry instance:{}"#,id))?
		{
			sql::Value::Object(o) => {Ok(Some(o))}
			_ => {Err(anyhow!(r#""file" in entry instance:{} is not an object"#,id))}
		}
	} else {Ok(None)}
}

pub async fn lookup_instance_filepath(id:&str) -> anyhow::Result<Option<PathBuf>>
{
	match lookup_instance_file(id).await.context("looking up fileinfo failed")?
	{
		Some(mut obj) => {
			let owned:bool = db::extract(&mut obj,"owned").expect(r#""owned" missing in file object"#);
			let path:String = db::extract(&mut obj,"path")?;
			if owned {
				Ok(Some(complete_filepath(path)))
			} else {
				Ok(Some(PathBuf::from(path)))
			}
		}
		None => Ok(None)
	}
}

pub fn map_extract<T>(map:&mut JsonMap, key:&str) -> anyhow::Result<T> where T:DeserializeOwned
{
	let value= map.remove(key).ok_or(anyhow!(r#"attr '{key}' is missing"#))?;
	serde_json::value::from_value(value)
		.context(format!(r#"Failed to interpret {} as {}"#,key,type_name::<T>()))
}
