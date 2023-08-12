use dicom::object::DefaultDicomObject;
pub use surrealdb::sql::Value as DbVal;
pub use serde_json::Value as JsonValue;
pub type JsonMap = serde_json::map::Map<String,JsonValue>;
use anyhow::Result;
use dicom::dictionary_std::tags;
use surrealdb::sql::Thing;

pub mod db;
pub mod dcm;
pub mod storage;
pub mod config;
pub mod tools;
pub mod server;

use dcm::extract;
use crate::dcm::{INSTACE_TAGS,SERIES_TAGS,STUDY_TAGS};

#[derive(Default)]
pub struct RegistryGuard(Option<Thing>);

impl RegistryGuard
{
	pub fn set(&mut self,id:Thing){self.0=Some(id);}
	pub fn reset(&mut self){self.0=None;}
}
impl Drop for RegistryGuard
{
	fn drop(&mut self) {
		if let Some(ref id)=self.0{
			tokio::spawn(db::unregister(id.clone()));//todo https://github.com/tokio-rs/tokio/issues/2289
		}
		self.0=None;
	}
}

pub async fn register_instance(obj:&DefaultDicomObject,add_meta:Vec<(String,DbVal)>,guard:Option<&mut RegistryGuard>) -> Result<JsonValue>
{
	let instance_id = obj.element(tags::SOP_INSTANCE_UID)?.to_str()?;
	let series_id = obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?;
	let study_id = obj.element(tags::STUDY_INSTANCE_UID)?.to_str()?;

	let instance_id_bak = Thing::from(("instances",instance_id.as_ref()));
	let instance_id:DbVal = instance_id_bak.clone().into();

	let series_id:DbVal = Thing::from(("series",series_id.as_ref())).into();
	let study_id:DbVal = Thing::from(("studies",study_id.as_ref())).into();

	let instance_meta = extract(&obj,INSTACE_TAGS.clone()).into_iter()
		.chain([("id".into(),instance_id),("series".into(),series_id.clone())])
		.chain(add_meta);
	let series_meta = extract(&obj, SERIES_TAGS.clone()).into_iter()
		.chain([("id".into(),series_id),("study".into(),study_id.clone())]);
	let study_meta = extract(&obj, STUDY_TAGS.clone()).into_iter()
		.chain([("id".into(),study_id)]);

	let res=db::register(instance_meta.collect(),series_meta.collect(),study_meta.collect()).await?;
	if res.is_null() { // we just created an entry, set the guard if provided
		if let Some(g) = guard {
			g.set(instance_id_bak);
		}
	}
	Ok(res)
}

