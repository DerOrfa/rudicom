pub mod store;
pub mod remove;
pub mod import;
pub mod verify;
mod error;

use crate::db;
use crate::db::RecordId;
use crate::tools::Error::DicomError;
use dicom::object::DefaultDicomObject;
pub use error::{Context, Error, Result, Source};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use surrealdb::sql;

pub fn transform(root:sql::Value, transformer:fn(sql::Value)->sql::Value) -> sql::Value
{
	match root {
		sql::Value::Array(a) => {
			let a:sql::Array=a.into_iter().map(|v|transform(v,transformer)).collect();
			sql::Value::Array(a)
		}
		sql::Value::Object(o) => {
			let o:BTreeMap<_,_>=o.into_iter().map(|(k,v)|(k,transform(v,transformer))).collect();
			sql::Object::from(o).into()
		}
		_ => transformer(root)
	}
}
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
/// generate absolute path using "storage_path" from the config if given path is relative
/// as "storage_path" is guaranteed to be absolute already, the result is guaranteed to be absolute
pub fn complete_filepath<P>(path:&P) -> PathBuf where P:AsRef<Path>
{
	crate::config::get::<PathBuf>("storage_path")
		.expect(r#""storage_path" missing or invalid in config"#)
		.join(path)
}
pub async fn get_instance_dicom(id:&str) -> Result<Option<DefaultDicomObject>>
{
	match lookup_instance_file(id).await
	{
		Ok(Some(file)) => Some(file.read().await).transpose(),
		Ok(None) => Ok(None),
		Err(e) => Err(e.context(format!("looking up fileinfo for {id}")))
	}
}
pub(crate) async fn lookup_instance_file<I>(id:I) -> Result<Option<db::File>> where surrealdb::RecordIdKey: From<I>
{
	let id = db::RecordId::instance(id);
	let id_str = id.to_string();
	if let Some(mut e)= db::lookup(id).await
		.map_err(|e|e.context(format!("failed looking for file in {id_str}")))?
	{
		let file = e.remove("file")
			.ok_or(Error::ElementMissing {element:"file".into(),parent:id_str})?
			.try_into()?;
		Ok(Some(file))
	} else {
		Ok(None)
	}
}

pub async fn instances_for_entry(id:RecordId) -> Result<Vec<String>>
{
	let context = format!("listing instances in {}",id);
	let res = match id.table() {
		"studies" => db::list_fields(id.clone(), "array::flatten(series.instances)").await,
		"series" => db::list_fields(id.clone(), "instances").await,
		"instances" => Ok(vec![id]),
		_ => Err(Error::InvalidTable {table:id.table().to_string()})
	}.map_err(|e|e.context(context))?;
	Ok(res.into_iter().map(|r|r.key().to_string()).collect())
}

pub fn extract_from_dicom(obj:&DefaultDicomObject,tag:dicom::core::Tag) -> Result<std::borrow::Cow<str>>
{
	obj
		.element(tag).map_err(|e|DicomError(e.into()))
		.and_then(|v|v.to_str().map_err(|e|DicomError(e.into())))
		.context(format!("getting {} from dicom object",tag))
}
