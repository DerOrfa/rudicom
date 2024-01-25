pub mod store;
pub mod remove;
pub mod import;
pub mod verify;
mod error;

use std::any::type_name;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
pub use remove::remove;
use crate::{db, storage};
pub use error::{Error, Result, Context, Source};
use crate::tools::Error::DicomError;

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
pub async fn get_instance_dicom(id:&str) -> Result<Option<DefaultDicomObject>>
{
	if let Some(file)=lookup_instance_file(id).await.map_err(|e|e.context(format!("looking up fileinfo for {id}")))?
	{
		let path = file.get_path();
		let checksum=file.md5.as_str();
		let mut md5=md5::Context::new();
		let obj=storage::async_store::read_file(&path,Some(&mut md5)).await?;
		if format!("{:x}", md5.compute()) == checksum{Ok(Some(obj))}
		else {Err(Error::ChecksumErr{checksum:checksum.into(),file:path.to_string_lossy().into()})}
	} else { Ok(None) }
}
pub(crate) async fn lookup_instance_file(id:&str) -> Result<Option<db::File>>
{
	let id = sql::Thing::from(("instances",id));
	if let Some(mut e)= db::lookup(&id).await.map_err(|e|e.context(format!("failed looking for file in {}", id)))?
	{
		let file = e.remove("file")
			.ok_or(Error::ElementMissing {element:"file".into(),parent:id.to_raw()})?
			.try_into()?;
		Ok(Some(file))
	} else {
		Ok(None)
	}
}

pub async fn lookup_instance_filepath(id:&str) -> Result<Option<PathBuf>>
{
	lookup_instance_file(id).await
		.map_err(|e|e.context(format!("looking up fileinfo for {id} failed")))
		.map(|f|f.map(|f|f.get_path()))
}

pub async fn instances_for_entry(id:sql::Thing) -> Result<Vec<sql::Thing>>
{
	let context = format!("listing instances in {}",id);
	match id.tb.as_str() {
		"studies" => db::query_for_list(&id,"series.instances").await,
		"series" => db::query_for_list(&id,"instances").await,
		"instances" => Ok(vec![id]),
		_ => Err(Error::InvalidTable {table:id.tb})
	}.map_err(|e|e.context(context))
}

pub fn extract_from_dicom(obj:&DefaultDicomObject,tag:dicom::core::Tag) -> Result<Cow<str>>
{
	obj
		.element(tag).map_err(|e|DicomError(e.into()))
		.and_then(|v|v.to_str().map_err(|e|DicomError(e.into())))
		.context(format!("getting {} from dicom object",tag))
}

pub(crate) fn extract_query_param<T>(params:&mut HashMap<String,String>, name:&str) -> Result<Option<T>>
	where
		T:FromStr,
		<T as FromStr>::Err: std::error::Error  + Send + Sync +'static
{
	params.remove("registered")
		.map(|s|s.parse::<T>()
			.map_err(|e|Error::ParseError {to_parse:s,source:e.into()})
		).transpose()
		.context(format!("extracting parameter {name} as {}",type_name::<T>()))
}

