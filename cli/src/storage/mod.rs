use std::path::Path;
use surrealdb::sql;
use crate::db;
use crate::tools::Context;

pub mod async_store;

pub(crate) async fn register_file<T>(path:T) -> crate::tools::Result<Option<db::Entry>> where T:AsRef<Path>
{
	let mut md5=md5::Context::new();

	let path_str = path.as_ref().to_str()
		.ok_or(crate::tools::Error::InvalidFilename {
			name:path.as_ref().to_path_buf()
		})?;
	let file = async_store::read_file(path.as_ref(),Some(&mut md5)).await
		.context(format!("reading {path_str}"))?;
	let md5:sql::Strand= format!("{:x}", md5.compute()).into();

	let fileinfo = sql::Object::try_from(db::File {
		path:path_str.into(),
		owned: false,
		md5:md5.clone(),
	})?;

	match db::register_instance(&file,vec![("file".into(),fileinfo.into())],None).await// push in our own md5 in case it differs
		.context(format!("registering {path_str}"))?
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
