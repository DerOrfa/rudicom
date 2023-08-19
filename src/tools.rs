pub mod store;
pub mod remove;
pub mod import;

use std::any::type_name;
use std::path::PathBuf;
use anyhow::{anyhow, Context};
use dicom::object::DefaultDicomObject;
use serde::de::DeserializeOwned;
use surrealdb::sql::Thing;
pub use remove::remove;
pub use store::store;
use crate::{JsonMap, storage};
use crate::dcm::complete_filepath;


pub async fn get_instance_dicom(id:&str) -> anyhow::Result<Option<DefaultDicomObject>>
{
	let mut map=lookup_instance_file(id).await.context("looking up fileinfo failed")?;
	if map.is_empty(){return Ok(None);}
	let path:String = map_extract(&mut map,"path")?;
	let checksum:String=map_extract(&mut map, "md5")?;
	let path=match map_extract(&mut map,"owned")? {
		true => complete_filepath(path),
		false => PathBuf::from(path)
	};
	let mut md5=md5::Context::new();
	let obj=storage::async_store::read_file(path,Some(&mut md5)).await?;
	if format!("{:x}", md5.compute()) == checksum{Ok(Some(obj))}
	else {Err(anyhow!(r#"found checksum '{}' doesn't fit the data"#,checksum))}
}
pub async fn lookup_instance_file(id:&str) -> anyhow::Result<JsonMap>
{
	use serde_json::Value::{Null,Object};
	let id = Thing::from(("instances",id));
	match crate::db::query_for_entry(id.clone()).await.context(format!("failed looking up {}",id))?
	{
		Null => {Ok(JsonMap::default())}
		Object(mut res) => {
			match res.remove("file").ok_or(anyhow!(r#""file" missing in entry instance:{}"#,id))?
			{
				Object(o) => {Ok(o)}
				_ => {Err(anyhow!(r#""file" in entry instance:{} is not an object"#,id))}
			}
		}
		_ => {Err(anyhow!(r#"Invalid database reply when looking up instance:{}"#,id))}
	}
}

pub async fn lookup_instance_filepath(id:&str) -> anyhow::Result<Option<PathBuf>>
{
	let mut map=lookup_instance_file(id).await.context("looking up fileinfo failed")?;
	if map.is_empty() {return Ok(None);}
	let owned:bool = map_extract(&mut map,"owned")?;
	let path:String = map_extract(&mut map,"path")?;
	if owned {
		Ok(Some(complete_filepath(path)))
	} else {
		Ok(Some(PathBuf::from(path)))
	}
}

pub fn map_extract<T>(map:&mut JsonMap, key:&str) -> anyhow::Result<T> where T:DeserializeOwned
{
	let value= map.remove(key).ok_or(anyhow!(r#"attr '{key}' is missing"#))?;
	serde_json::value::from_value(value)
		.context(format!(r#"Failed to interpret {} as {}"#,key,type_name::<T>()))
}

