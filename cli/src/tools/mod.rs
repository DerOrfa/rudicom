pub mod store;
pub mod remove;
pub mod import;
pub mod verify;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
pub use remove::remove;
use crate::{db, storage};
use crate::db::DBErr;

pub fn transform(root:sql::Value, transformer:fn(sql::Value)->sql::Value) -> sql::Value
{
	match root {
		sql::Value::Array(a) => {
			let a:sql::Array=a.into_iter().map(|v|transform(v,transformer)).collect();
			sql::Value::Array(a)
		}
		sql::Value::Object(o) => {
			let o:BTreeMap<_,_>=o.into_iter().map(|(k,v)|(k,transform(v,transformer))).collect();
			sql::Object(o).into()
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

pub fn complete_filepath<P>(path:&P) -> PathBuf where P:AsRef<Path>
{
	let root:PathBuf = crate::config::get("storage_path").expect(r#""storage_path" missing or invalid in config"#);
	root.join(path)
}
pub async fn get_instance_dicom(id:&str) -> Result<Option<DefaultDicomObject>,DBErr>
{
	if let Some(file)=lookup_instance_file(id).await.map_err(|e|e.context(format!("looking up fileinfo for {id}")))?
	{
		let path = file.get_path();
		let checksum=file.md5.as_str();
		let mut md5=md5::Context::new();
		let obj=storage::async_store::read_file(&path,Some(&mut md5)).await?;
		if format!("{:x}", md5.compute()) == checksum{Ok(Some(obj))}
		else {Err(DBErr::ChecksumErr{checksum:checksum.into(),file:path.to_string_lossy().into()})}
	} else { Ok(None) }
}
pub(crate) async fn lookup_instance_file(id:&str) -> Result<Option<db::File>,DBErr>
{
	let id = sql::Thing::from(("instances",id));
	if let Some(mut e)= db::lookup(&id).await.map_err(|e|e.context(format!("failed looking for file in {}", id)))?
	{
		let file = e.remove("file")
			.ok_or(DBErr::ElementMissing {element:"file".into(),parent:id.to_raw()})?
			.try_into()?;
		Ok(Some(file))
	} else {
		Ok(None)
	}
}

pub async fn lookup_instance_filepath(id:&str) -> Result<Option<PathBuf>,DBErr>
{
	lookup_instance_file(id).await
		.map_err(|e|e.context(format!("looking up fileinfo for {id} failed")))
		.map(|f|f.map(|f|f.get_path()))
}

pub async fn instances_for_entry(id:sql::Thing) -> Result<Vec<sql::Thing>,DBErr>
{
	let context = format!("listing instances in {}",id);
	match id.tb.as_str() {
		"studies" => db::query_for_list(&id,"series.instances").await,
		"series" => db::query_for_list(&id,"instances").await,
		"instances" => Ok(vec![id]),
		_ => Err(DBErr::InvalidTable {table:id.tb})
	}.map_err(|e|e.context(context))
}

pub fn extract_from_dicom(obj:&DefaultDicomObject,tag:dicom::core::Tag) -> Result<Cow<str>,DBErr>
{
	obj
		.element(tag).map_err(DBErr::from)
		.and_then(|v|v.to_str().map_err(DBErr::from))
		.map_err(|e|e.context(format!("getting {} from dicom object",tag)))
}
