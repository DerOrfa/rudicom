use std::collections::BTreeMap;
use std::path::Path;
use anyhow::anyhow;
use md5::Context;
use crate::{DbVal, JsonVal, register_instance};

pub mod async_store;

pub async fn register_file<T>(path:T) -> anyhow::Result<JsonVal> where T:AsRef<Path>
{
	let mut md5=Context::new();
	let file = async_store::read_file(path.as_ref(),Some(&mut md5)).await?;

	let path = path.as_ref().to_str().ok_or(anyhow!("Failed to encode filename in UTF-8"))?;

	let fileinfo:BTreeMap<String,DbVal>= BTreeMap::from([
		("path".into(),path.into()),
		("owned".into(),false.into()),
		("md5".into(),format!("{:x}", md5.compute()).into())
	]);
	register_instance(&file,vec![("file".into(),fileinfo.into())],None).await
}
