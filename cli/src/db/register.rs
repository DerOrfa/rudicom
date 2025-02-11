use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;
use dicom::core::Tag;
use surrealdb::Value;

use crate::db;
use crate::db::{lookup, Entry, RecordId, DB};
use crate::dcm;
use crate::tools::extract_from_dicom;
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use dcm::AttributeSelector;

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
			let del = DB.delete(id);
			tokio::spawn(async { del.await });//todo https://github.com/tokio-rs/tokio/issues/2289
		}
		self.0=None;
	}
}

async fn insert<'a>(
	obj:&DefaultDicomObject,
	record_id: RecordId,
	add_meta:Vec<(&'a str,Value)>,
	tags:&HashMap<String,Vec<AttributeSelector>>
) -> crate::tools::Result<Value>
{
	let meta: BTreeMap<_, _> = 
		dcm::extract(&obj, &tags).into_iter()
		.chain([("uid", Value::from_inner(record_id.str_key().into()))])
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect();
	match record_id.table()
	{
		// colliding inserts of instances will result in no insert and Err,
		"instances" => DB.insert(record_id).content(meta).await,
		// others will silently overwrite the data (it's the same anyway, and we don't need to know)  
		_ => 	DB.upsert(record_id).content(meta).await,
	}.map_err(|e|e.into())
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
	pub static INSTANCE_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> = 
		LazyLock::new(|| dcm::get_attr_list(db::Table::Instances, vec![("Number", vec![Tag::from((0x0020,0x0013))])]));//InstanceNumber
	pub static SERIES_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> = 
		LazyLock::new(|| dcm::get_attr_list(db::Table::Series, vec![
			("Description",vec![Tag::from((0x0008,0x103E))]), //SeriesDescription
			("Number",vec![Tag::from((0x0020,0x0011))]) // SeriesNumber
		])
	);
	pub static STUDY_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> = 
		LazyLock::new(|| dcm::get_attr_list(db::Table::Studies, vec![
			("Name",vec![Tag::from((0x0010,0x0010))]),//PatientName
			("Time", vec![Tag::from((0x0008,0x0030))]), // StudyTime 
			("Date", vec![Tag::from((0x0008,0x0020))]) // StudyDate
		])
	);

	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;

	let instance_id = RecordId::from_instance(instance_uid.as_ref(), series_uid.as_ref(), study_uid.as_ref());

	// try to avoid insert operation as much as possible
	if let Some(existing) = lookup(instance_id.clone()).await?{
		Ok(Some(existing)) // return already existing entry
	} else {
		let series_id= RecordId::from_series(&series_uid,&study_uid);
		if DB.select::<Value>(&series_id).await?.into_inner().is_none() {
			let study_id = RecordId::from_study(&study_uid);
			if DB.select::<Value>(&study_id).await?.into_inner().is_none() {
				insert(obj, study_id, vec![], &STUDY_TAGS).await?;
			}
			insert(obj, series_id, vec![], &SERIES_TAGS).await?;
		}
		let inserted:Entry = insert(obj, instance_id.clone(), add_meta, &INSTANCE_TAGS).await?.try_into()?;
		// successfully inserted
		// set up the guard with the registered instance, so we can roll back the registration if needed
		if let Some(guard) = guard{
			guard.set(inserted.id().clone());
		}
		Ok(None) // return None (as opposed to returning already existing entry)
	}
}
