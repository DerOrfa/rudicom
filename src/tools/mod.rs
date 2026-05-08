pub mod store;
pub mod remove;
pub mod import;
pub mod verify;
mod error;
pub mod conv;
pub mod tar;

use crate::db;
use crate::db::{lookup_uid, Pickable, RecordId, DB};
use crate::tools::Error::{DicomError, NotFound};
use dicom::object::DefaultDicomObject;
pub use error::{Context, Error, Result, Source};
use std::path::{Path, PathBuf};
use surrealdb::types as db_types;

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
	let ctx = format!("listing children of {}",id);
	let my_table = id.table.as_str();
	let mut me= db::lookup(id).await?.ok_or(NotFound)?;
	let mut ret = vec![];
	let values = match (my_table, table){
		("studies","instances") => { // grandchildren need special query
			DB.query("select array::flatten(series.instances) as series from $rec").bind(("rec",id.0.to_owned()))
				.await?.take::<Option<Vec<db_types::Value>>>("series")?.ok_or(NotFound)?
				.into_iter()
		},
		// every entry knows its children anyway
		("series","instances") => me.pick_remove("instances")?.into_array()?.into_iter(),
		("studies","series") => me.pick_remove("series")?.into_array()?.into_iter(),
		_ => {return Ok(vec![me])}
	};
	for v in values
	{
		ret.push(db::lookup(&RecordId(v.into_record()?)).await?.expect("failed accessing children of study"));
	}
	Ok(ret)

}

pub fn extract_from_dicom(obj:&'_ DefaultDicomObject,tag:dicom::core::Tag) -> Result<std::borrow::Cow<'_, str>>
{
	obj
		.element(tag).map_err(|e|DicomError(e.into()))
		.and_then(|v|v.to_str().map_err(|e|DicomError(e.into())))
		.context(format!("getting {} from dicom object",tag))
}
