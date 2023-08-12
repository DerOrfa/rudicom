pub mod store;
pub mod remove;

use std::any::type_name;
use std::path::PathBuf;
use anyhow::{anyhow, Context};
use serde::de::DeserializeOwned;
use surrealdb::sql::Thing;
pub use remove::remove;
pub use store::store;
use crate::{JsonMap, storage};
use crate::dcm::complete_filepath;

pub async fn lookup_instance_file(id:&str) -> anyhow::Result<JsonMap>
{
	use serde_json::Value::{Null,Object};
	let id = Thing::from(("instances",id));
	match crate::db::query_for_entry(id.clone()).await.context(format!("failed looking up {}",id))?
	{
		Null => {Err(anyhow!(r#""file" missing in entry instance:{}"#,id))}
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

pub async fn lookup_instance_filepath(id:&str) -> anyhow::Result<PathBuf>
{
	let mut map=lookup_instance_file(id).await.context("looking up fileinfo failed")?;
	let owned:bool = map_extract(&mut map,"owned")?;
	let path:String = map_extract(&mut map,"path")?;
	if owned {
		Ok(complete_filepath(path))
	} else {
		Ok(PathBuf::from(path))
	}
}

pub fn map_extract<T>(map:&mut JsonMap, key:&str) -> anyhow::Result<T> where T:DeserializeOwned
{
	let value= map.remove(key).ok_or(anyhow!(r#"attr '{key}' is missing"#))?;
	serde_json::value::from_value(value)
		.context(format!(r#"Failed to interpret {} as {}"#,key,type_name::<T>()))
}

