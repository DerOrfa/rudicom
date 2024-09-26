use std::collections::BTreeMap;
use std::sync::LazyLock;
use surrealdb::{sql, Error};

use crate::db;
use crate::db::{RecordId, DB};
use crate::dcm;
use crate::tools::extract_from_dicom;
use dicom::dictionary_std::tags;
use dicom::object::{DefaultDicomObject, Tag};
use surrealdb::error::Api::Query;
use surrealdb::sql::Value;
use surrealdb::Result;

/// register a new instance using values in instance_meta
/// if the series and study referred to in instance_meta do not exist already
/// they are created using series_meta and study_meta
/// if the instance exists already no change is done and the existing instance data is returned
/// None otherwise (on a successful register)
pub async fn register(
	instance_meta:BTreeMap<String, Value>,
	series_meta:BTreeMap<String, Value>,
	study_meta:BTreeMap<String, Value>)
-> Result<Value>
{
	loop {
		let mut res= DB
			.query("fn::register($instance_meta, $series_meta, $study_meta)")
			.bind(("instance_meta",instance_meta.clone()))
			.bind(("series_meta",series_meta.clone()))
			.bind(("study_meta",study_meta.clone()))
			.await?;
		let errors= res.take_errors();
		if let Some((_,last))= errors.into_iter().last() {
			return Err(last);
		}
		return res.take::<surrealdb::Value>(0).map(|r|r.into_inner().first())
	}
}

pub async fn unregister(id:RecordId) -> Result<sql::Value>
{
	let r:Option<surrealdb::Value> = DB.delete(id.0).await?;
	dbg!(r);
	todo!()
}

#[derive(Default)]
pub struct RegistryGuard(Option<RecordId>);

impl RegistryGuard
{
	pub fn set(&mut self,id:RecordId){self.0=Some(id);}
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
pub async fn register_instance<'a>(
	obj:&DefaultDicomObject,
	add_meta:Vec<(&'a str,db::Value)>,
	guard:Option<&mut RegistryGuard>
) -> crate::tools::Result<Option<db::Entry>> {
	pub static INSTANCE_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("instance_tags", vec!["InstanceNumber"]));
	pub static SERIES_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("series_tags", vec!["SeriesDescription", "SeriesNumber"]));
	pub static STUDY_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("study_tags", vec!["PatientID", "StudyTime", "StudyDate"]));

	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;
	
	let study_id =  RecordId::study(study_uid.as_ref());
	let series_id = RecordId::series(series_uid.as_ref(), study_uid.as_ref());
	let instance_id = RecordId::instance(instance_uid.as_ref(), series_uid.as_ref(), study_uid.as_ref());

	let instance_meta: BTreeMap<_, _> = dcm::extract(&obj, &INSTANCE_TAGS).into_iter()
		.chain([("id", instance_id.clone().into())])
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	let series_meta: BTreeMap<_, _> = dcm::extract(&obj, &SERIES_TAGS).into_iter()
		.chain([("id", series_id.into())])
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	let study_meta: BTreeMap<_, _> = dcm::extract(&obj, &STUDY_TAGS).into_iter()
		.chain([("id", study_id.into()),])
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	loop {
		match register(instance_meta.clone(), series_meta.clone(), study_meta.clone()).await
		{
			Ok(v) => {
				return if v.is_some() { // we just created an entry, set the guard if provided
					Ok(Some(db::Entry::try_from(v)?))
				} else { // data already existed - no data stored - return existing data
					if let Some(g) = guard { g.set(instance_id); }
					Ok(None) //and return None existing entry
				}
			}
			Err(Error::Api(Query(message))) => {
				if message == "The query was not executed due to a failed transaction. There was a problem with a datastore transaction: Transaction read conflict" {
					continue // retry
				} else {
					return Err(Error::Api(Query(message)).into())
				}
			},
			Err(e) => return Err(e.into())
		}
	}

}
