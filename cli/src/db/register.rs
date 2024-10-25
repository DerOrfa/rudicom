use std::collections::BTreeMap;
use std::sync::LazyLock;
use surrealdb::Value;

use crate::db;
use crate::db::{lookup, RecordId, DB};
use crate::dcm;
use crate::tools::extract_from_dicom;
use dicom::dictionary_std::tags;
use dicom::object::{DefaultDicomObject, Tag};
use surrealdb::opt::Resource;
use surrealdb::Result;

pub async fn unregister(id:RecordId) -> Result<Value>
{
	DB.delete(surrealdb::opt::Resource::from(id.0)).await
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

async fn insert<'a>(
	obj:&DefaultDicomObject,
	record_id: RecordId,
	add_meta:Vec<(&'a str,Value)>,
	tags:&Vec<(String,Tag)>
) -> crate::tools::Result<Option<Value>>
{
	let series_meta: BTreeMap<_, _> = dcm::extract(&obj, &tags).into_iter()
		.chain([("uid", Value::from_inner(record_id.str_key().into()))])
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	let id = Resource::from(record_id.0);
	let res = DB.insert::<Value>(id.clone()).content(series_meta).await;
	match res
	{
		Ok(v) => Ok(None),
		Err(surrealdb::Error::Api(surrealdb::error::Api::Query(s))) => {
			let existing = DB.select::<Value>(id).await?;
			if existing.into_inner_ref().is_some(){
				Ok(Some(existing))
			} else {Err(surrealdb::Error::Api(surrealdb::error::Api::Query(s)).into())}
		},
		Err(e) => Err(e.into()),
	}
}
/// register dicom object of an instance
/// if the instance already exists no change is done and
/// the existing instance is returned as Entry
/// None is returned on a successful register
pub async fn register_instance<'a>(
	obj:&DefaultDicomObject,
	add_meta:Vec<(&'a str,Value)>,
	guard:Option<&mut RegistryGuard>
) -> crate::tools::Result<Option<db::Entry>> {
	pub static INSTANCE_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("instance_tags", vec!["InstanceNumber"]));
	pub static SERIES_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("series_tags", vec!["SeriesDescription", "SeriesNumber"]));
	pub static STUDY_TAGS: LazyLock<Vec<(String, Tag)>> = LazyLock::new(|| dcm::get_attr_list("study_tags", vec!["PatientID", "StudyTime", "StudyDate"]));

	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;

	let instance_id = RecordId::from_instance(instance_uid.as_ref(), series_uid.as_ref(), study_uid.as_ref());

	// try to avoid insert operation as much as possible
	if let Some(existing) = lookup(instance_id.clone()).await?{
		Ok(Some(existing))
	} else {
		let series_id= RecordId::from_series(&series_uid,&study_uid);
		if DB.select::<Value>(Resource::from(&series_id.0)).await?.into_inner().is_none() {
			let study_id = RecordId::from_study(&study_uid);
			if DB.select::<Value>(Resource::from(&study_id.0)).await?.into_inner().is_none() {
				insert(obj, study_id, vec![], &STUDY_TAGS).await?;
			}
			insert(obj, series_id, vec![], &SERIES_TAGS).await?;
		}
		match insert(obj, instance_id.clone(), add_meta, &INSTANCE_TAGS).await?
		{
			None => { // ok actually stored (didn't exist already)
				//set up the guard with the registered instance, so we can roll back the registry if needed
				if let Some(guard) = guard{
					guard.set(instance_id);
				}
				Ok(None)
			}
			// actually there was an instance after all (race condition)
			Some(existing) => Some(db::Entry::try_from(existing)).transpose()
		}
	}
}
