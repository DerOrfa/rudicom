use std::collections::BTreeMap;
use std::sync::OnceLock;
use surrealdb::Result;
use surrealdb::opt::IntoQuery;
use surrealdb::sql;

use dicom::object::{DefaultDicomObject, Tag};
use dicom::dictionary_std::tags;
use crate::db;
use crate::dcm;

static INSERT_STUDY:OnceLock<Vec<sql::Statement>> = OnceLock::new();
static INSERT_SERIES:OnceLock<Vec<sql::Statement>> = OnceLock::new();
static INSERT_INSTANCE:OnceLock<Vec<sql::Statement>> = OnceLock::new();

pub async fn register(
	instance_meta:BTreeMap<String, sql::Value>,
	series_meta:BTreeMap<String, sql::Value>,
	study_meta:BTreeMap<String, sql::Value>)
-> Result<sql::Value>
{
	let ins_study= INSERT_STUDY.get_or_init(||"INSERT INTO studies $study_meta return before".into_query().unwrap());
	let ins_series = INSERT_SERIES.get_or_init(||"INSERT INTO series $series_meta return before".into_query().unwrap());
	let ins_inst = INSERT_INSTANCE.get_or_init(||"INSERT INTO instances $instance_meta return before".into_query().unwrap());
	let mut res= super::db()
		.query(ins_study.clone())
		.query(ins_series.clone())
		.query(ins_inst.clone())
		.bind(("instance_meta",instance_meta))
		.bind(("series_meta",series_meta))
		.bind(("study_meta",study_meta))
		.await?.check()?;

	res.take::<sql::Value>(2).map(|r|r.first())
}

pub async fn unregister(id:sql::Thing) -> Result<sql::Value>
{
	Ok(super::db().delete(id).await?.unwrap_or(sql::Value::None))
}

#[derive(Default)]
pub struct RegistryGuard(Option<db::Thing>);

impl RegistryGuard
{
	pub fn set(&mut self,id:db::Thing){self.0=Some(id);}
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


pub async fn register_instance(
	obj:&DefaultDicomObject,
	add_meta:Vec<(String,db::Value)>,
	guard:Option<&mut RegistryGuard>
) -> anyhow::Result<Option<db::Entry>>
{
	pub static INSTANCE_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();
	pub static SERIES_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();
	pub static STUDY_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();


	let instance_id = obj.element(tags::SOP_INSTANCE_UID)?.to_str()?;
	let series_id = obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?;
	let study_id = obj.element(tags::STUDY_INSTANCE_UID)?.to_str()?;

	let instance_id_bak = db::Thing::from(("instances",instance_id.as_ref()));
	let instance_id:db::Value = instance_id_bak.clone().into();

	let series_id:db::Value = db::Thing::from(("series",series_id.as_ref())).into();
	let study_id:db::Value = db::Thing::from(("studies",study_id.as_ref())).into();

	let instance_tags= INSTANCE_TAGS.get_or_init(||dcm::get_attr_list("instance_tags", vec!["InstanceNumber"]));
	let series_tags = SERIES_TAGS.get_or_init(||dcm::get_attr_list("series_tags",vec!["SeriesDescription", "SeriesNumber"]));
	let study_tags = STUDY_TAGS.get_or_init(||dcm::get_attr_list("study_tags", vec!["PatientID", "StudyTime", "StudyDate"]));

	let instance_meta:BTreeMap<_,_> = dcm::extract(&obj, instance_tags.clone()).into_iter()
		.chain([("id".into(),instance_id),("series".into(),series_id.clone())])
		.chain(add_meta).collect();
	let series_meta:BTreeMap<_,_> = dcm::extract(&obj, series_tags.clone()).into_iter()
		.chain([("id".into(),series_id),("study".into(),study_id.clone())])
		.collect();
	let study_meta:BTreeMap<_,_> = dcm::extract(&obj, study_tags.clone()).into_iter()
		.chain([("id".into(),study_id)])
		.collect();

	let res=register(instance_meta,series_meta,study_meta).await?;
	if res.is_some() { // we just created an entry, set the guard if provided
		Some(res.try_into()).transpose()
	} else { // data already existed - no data stored - return existing data
		if let Some(g) = guard {
			g.set(instance_id_bak);
		}
		Ok(None) //and return None existing entry
	}

}
