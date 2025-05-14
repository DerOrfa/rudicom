pub mod store;
pub mod remove;
pub mod import;
pub mod verify;
mod error;
pub mod conv;

use crate::db;
use crate::db::{lookup_uid, Entry, RecordId, DB};
use crate::tools::Error::DicomError;
use dicom::object::DefaultDicomObject;
pub use error::{Context, Error, Result, Source};
use std::iter::repeat;
use std::ops::Bound::Included;
use std::path::{Path, PathBuf};
use surrealdb::opt::Resource;
use surrealdb::sql;

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
	crate::config::get().paths.storage_path.join(path)
}
pub async fn get_instance_dicom(id:String) -> Result<Option<DefaultDicomObject>>
{
	match lookup_instance_file(id).await
	{
		Ok(Some(file)) => Some(file.read().await).transpose(),
		Ok(None) => Ok(None),
		Err(e) => Err(e)
	}
}
pub async fn lookup_instance_file(id:String) -> Result<Option<db::File>>
{
	let ctx = format!("failed looking for file for instance {id}");
	match lookup_uid("instances",id).await
	{
		Ok(Some(e)) => Some(e.get_file()).transpose(),
		Ok(None) => Ok(None),
		Err(e) => Err(e) 
	}.context(ctx)
}

pub async fn entries_for_record(id:&RecordId,table:&str) -> Result<Vec<db::Entry>>
{
	let size = match table { 
		"instances" => 18,
		"series" => 12,
		_ => unreachable!()
	};
	let ctx = format!("listing children of {}",id);
	let id_vec= id.key_vec().to_vec();
	let max_gen = repeat(i64::MAX).map(sql::Value::from).take(size-id_vec.len());
	let min_gen = repeat(i64::MIN).map(sql::Value::from).take(size-id_vec.len());

	
	let begin:Vec<_> = id_vec.iter().map(|v|v.clone()).chain(min_gen)
		.map(surrealdb::Value::from_inner).collect();
	let end:Vec<_> = id_vec.into_iter().chain(max_gen)
		.map(surrealdb::Value::from_inner).collect();
	let results = DB.select::<surrealdb::Value>(Resource::Table(table.to_string()))
		.range((Included(begin),Included(end))).await?.into_inner();
	if let sql::Value::Array(instances) = results {
		instances.0.into_iter().map(surrealdb::Value::from_inner).map(Entry::try_from)
			.collect::<Result<Vec<_>>>().context(ctx)
	} else {
		Err(Error::UnexpectedResult {expected:"list of entries".into(),found: results.kindof()})
	}
}

pub fn extract_from_dicom(obj:&DefaultDicomObject,tag:dicom::core::Tag) -> Result<std::borrow::Cow<str>>
{
	obj
		.element(tag).map_err(|e|DicomError(e.into()))
		.and_then(|v|v.to_str().map_err(|e|DicomError(e.into())))
		.context(format!("getting {} from dicom object",tag))
}
