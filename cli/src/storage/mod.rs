use std::path::PathBuf;
use surrealdb::sql;
use crate::db;
use crate::tools::Context;

pub mod async_store;

pub(crate) async fn register_file<T>(path:T) -> crate::tools::Result<Option<db::Entry>> where PathBuf:From<T>
{
	let mut md5=md5::Context::new();
	let path = PathBuf::from(path);

	// let path_str = path.as_ref().to_str()
	// 	.ok_or(InvalidFilename {name:path.as_ref().to_path_buf()})?;
	let file = async_store::read_file(&path,Some(&mut md5)).await
		.context(format!("reading {}",path.to_string_lossy()))?;

	let fileinfo = db::File::from_unowned(&path,md5.compute())
		.context(format!("creating fileinfo for {}",path.to_string_lossy()))?;
	let md5 = fileinfo.get_md5();
	let fileinfo_obj = sql::Object::try_from(fileinfo).unwrap();

	match db::register_instance(&file,vec![("file".into(), fileinfo_obj.into())],None).await// push in our own md5 in case it differs
		.context(format!("registering {}",path.to_string_lossy()))?
	{
		None => Ok(None),
		Some(mut existing) => {
			if existing.get_file()?.get_md5() != md5
			{
				existing.insert("conflicting_md5",md5);
			}
			Ok(Some(existing))
		}
	}
}
