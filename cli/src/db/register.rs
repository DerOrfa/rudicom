use std::collections::BTreeMap;
use std::sync::OnceLock;
use surrealdb::opt::IntoQuery;
use surrealdb::{sql, RecordId};

use dicom::object::{DefaultDicomObject, Tag};
use dicom::dictionary_std::tags;
use crate::db;
use crate::dcm;
use crate::tools::extract_from_dicom;
use surrealdb::Result;

static INSERT_STUDY:OnceLock<Vec<sql::Statement>> = OnceLock::new();
static INSERT_SERIES:OnceLock<Vec<sql::Statement>> = OnceLock::new();
static INSERT_INSTANCE:OnceLock<Vec<sql::Statement>> = OnceLock::new();
static DELETE:OnceLock<Vec<sql::Statement>> = OnceLock::new();

/// register a new instance using values in instance_meta
/// if the series an study referred to in instance_meta do not exist already
/// they are created using series_meta and study_meta
/// if the instance exists already no change is done and the existing instance data is returned
/// None otherwise (on a successful register)
pub async fn register(
	instance_meta:BTreeMap<&str, sql::Value>,
	series_meta:BTreeMap<&str, sql::Value>,
	study_meta:BTreeMap<&str, sql::Value>)
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

	res.take::<surrealdb::Value>(2).map(|r|r.into_inner().first())
}

pub async fn unregister(id:RecordId) -> Result<sql::Value>
{
	let del = DELETE.get_or_init(||"DELETE ONLY $id return $before".into_query().unwrap());
	super::db().query(del.clone()).bind(("id",id)).await?
		.check()?.take("before")
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
			tokio::spawn(unregister(id.clone()));//todo https://github.com/tokio-rs/tokio/issues/2289
		}
		self.0=None;
	}
}

/// register dicom object of an instance
/// if the instance already exists no change is done and
/// the existing instance is returned as Entry
/// None is returned on a successful register
pub async fn register_instance(
	obj:&DefaultDicomObject,
	add_meta:Vec<(&str,db::Value)>,
	guard:Option<&mut RegistryGuard>
) -> crate::tools::Result<Option<db::Entry>>
{
	pub static INSTANCE_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();
	pub static SERIES_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();
	pub static STUDY_TAGS:OnceLock<Vec<(String, Tag)>> = OnceLock::new();


	let instance_id = extract_from_dicom(obj,tags::SOP_INSTANCE_UID)?;
	let series_id =  extract_from_dicom(obj,tags::SERIES_INSTANCE_UID)?;
	let study_id =  extract_from_dicom(obj,tags::STUDY_INSTANCE_UID)?;

	let instance_id_bak = db::Thing::from(("instances",instance_id.as_ref()));
	let instance_id:db::Value = instance_id_bak.clone().into();

	let series_id:db::Value = db::Thing::from(("series",series_id.as_ref())).into();
	let study_id:db::Value = db::Thing::from(("studies",study_id.as_ref())).into();

	let instance_tags= INSTANCE_TAGS.get_or_init(||dcm::get_attr_list("instance_tags", vec!["InstanceNumber"]));
	let series_tags = SERIES_TAGS.get_or_init(||dcm::get_attr_list("series_tags",vec!["SeriesDescription", "SeriesNumber"]));
	let study_tags = STUDY_TAGS.get_or_init(||dcm::get_attr_list("study_tags", vec!["PatientID", "StudyTime", "StudyDate"]));

	let instance_meta = dcm::extract(&obj, instance_tags).into_iter()
		.chain([("id",instance_id),("series",series_id.clone())])
		.chain(add_meta)
		.collect();

	let series_meta= dcm::extract(&obj, series_tags).into_iter()
		.chain([("id",series_id),("study",study_id.clone())])
		.collect();

	let study_meta = dcm::extract(&obj, study_tags).into_iter()
		.chain([("id",study_id)])
		.collect();

	let res=register(instance_meta,series_meta,study_meta).await?;
	if res.is_some() { // we just created an entry, set the guard if provided
		Ok(Some(db::Entry::try_from(res)?))
	} else { // data already existed - no data stored - return existing data
		if let Some(g) = guard {
			g.set(instance_id_bak);
		}
		Ok(None) //and return None existing entry
	}

}
