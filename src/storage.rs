use std::path::Path;
use anyhow::anyhow;
use surrealdb::sql;
use crate::db;

pub mod async_store;

pub(crate) async fn register_file<T>(path:T) -> anyhow::Result<Option<db::Entry>> where T:AsRef<Path>
{
	let mut md5=md5::Context::new();
	let dcm = async_store::read_file(path.as_ref(),Some(&mut md5)).await?;
	let md5:sql::Strand= format!("{:x}", md5.compute()).into();

	let path = path.as_ref().to_str().ok_or(anyhow!("Failed to encode filename in UTF-8"))?.to_string();

	let fileinfo = sql::Object::try_from(db::File {
		path:path.into(),
		owned: false,
		md5:md5.clone(),
	})?;

	match db::register_instance(&dcm,vec![("file".into(),sql::Value::Object(fileinfo))],None).await? // push in our own md5 in case it differs
	{
		None => Ok(None),
		Some(mut existing) => {
			if existing.get_file()?.md5 != md5
			{
				existing.insert("conflicting_md5",md5);
			}
			Ok(Some(existing))
		}
	}
}
