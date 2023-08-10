use dicom::object::DefaultDicomObject;
use crate::db::{DbVal, JsonValue};
use anyhow::Result;
use dicom::dictionary_std::tags;
use surrealdb::sql::Thing;

pub mod db;
pub mod dcm;
pub mod file;
pub mod config;

use dcm::extract;
use crate::dcm::{INSTACE_TAGS,SERIES_TAGS,STUDY_TAGS};

pub async fn register_instance(obj:DefaultDicomObject,add_meta:Vec<(String,DbVal)>) -> Result<JsonValue>
{
	let instance_id = obj.element(tags::SOP_INSTANCE_UID)?.to_str()?;
	let series_id = obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?;
	let study_id = obj.element(tags::STUDY_INSTANCE_UID)?.to_str()?;

	let instance_id:DbVal = Thing::from(("instances",instance_id.as_ref())).into();
	let series_id:DbVal = Thing::from(("series",series_id.as_ref())).into();
	let study_id:DbVal = Thing::from(("studies",study_id.as_ref())).into();

	let instance_meta = extract(&obj,INSTACE_TAGS.clone()).into_iter()
		.chain([("id".into(),instance_id),("series".into(),series_id.clone())])
		.chain(add_meta);
	let series_meta = extract(&obj, SERIES_TAGS.clone()).into_iter()
		.chain([("id".into(),series_id),("study".into(),study_id.clone())]);
	let study_meta = extract(&obj, STUDY_TAGS.clone()).into_iter()
		.chain([("id".into(),study_id)]);
	db::register(instance_meta.collect(),series_meta.collect(),study_meta.collect())
		.await.map_err(|e|e.into())
}

