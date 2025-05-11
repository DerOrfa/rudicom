use std::collections::{BTreeMap, HashMap};
use surrealdb::Value;

use crate::db::{lookup, RecordId, RegisterResult, DB};
use crate::dcm::{INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::{extract_from_dicom, Error};
use crate::{dcm, tools};
use dcm::AttributeSelector;
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use surrealdb::error::Db::{QueryNotExecutedDetail, RecordExists};

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
) -> tools::Result<bool>
{
	let meta: BTreeMap<_, _> = 
		dcm::extract(&obj, &tags).into_iter()
		.chain([("uid", Value::from_inner(record_id.str_key().into()))])
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	loop {
		match DB.insert(&record_id).content(meta.clone()).await { 
			Err(surrealdb::Error::Db(RecordExists {thing})) => {
				if let Some(existing) = lookup(&RecordId(surrealdb::RecordId::from_inner(thing))).await?{
					return if existing == *obj {Ok(false)} 
						else {Err(Error::DataConflict(existing))}
				} // gone again??, try to repeat insert
			},
			Err(surrealdb::Error::Db(QueryNotExecutedDetail{message})) => {
				if message != "Failed to commit transaction due to a read or write conflict. This transaction can be retried"
				{
					return Err(surrealdb::Error::Db(QueryNotExecutedDetail{message}).into())
				} // race condition, try again
			}
			Err(e) => return Err(e.into()),
			Ok(_) => return Ok(true),
		}
	}
}

/// Register a dicom object of an instance.
/// 
/// If the instance already exists and is equal, no change is done and Ok(false) is returned
/// 
/// ## Arguments 
/// 
/// * `obj`: the dicom instance to be registered 
/// * `add_meta`: additional key value pairs to be added to the entry
/// * `guard`: optional guard that can be roll back the registration of Drop if not confirmed
/// 
/// ## returns: 
/// * Ok(true) if the instance was registered
/// * Ok(false) if the instance already exists and is equal
/// * Err(ExistingDifferent(Entry)) if the instance was already registered and different
/// * Error(tools::Error) if another error occurred 
/// 
pub async fn register_instance<'a>(
	obj:&DefaultDicomObject,
	add_meta:Vec<(&'a str,Value)>,
	guard:Option<&mut RegistryGuard>
) -> tools::Result<RegisterResult> {

	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;

	let instance_id = RecordId::from_instance(instance_uid.as_ref(), series_uid.as_ref(), study_uid.as_ref());

	// try to avoid insert operation as much as possible
	if let Some(existing) = lookup(&instance_id).await?{
		if existing!=*obj {
			Err(tools::Error::DataConflict(existing))
		} else { 
			Ok(RegisterResult::AlreadyStored(instance_id)) 
		}
	} else {
		let series_id= RecordId::from_series(&series_uid,&study_uid);
		if let Some(existing) = lookup(&series_id).await? {
			if existing!=*obj { return Err(Error::DataConflict(existing)) }
		} else {
			let study_id = RecordId::from_study(&study_uid);
			if let Some(existing) = lookup(&study_id).await? {
				if existing!=*obj { return Err(Error::DataConflict(existing)) }
			} else {
				insert(obj, study_id, vec![], &STUDY_TAGS).await?;
			}
			insert(obj, series_id, vec![], &SERIES_TAGS).await?;
		}
		if insert(obj, instance_id.clone(), add_meta, &INSTANCE_TAGS).await?{
			// successfully inserted
			// set up the guard with the registered instance, so we can roll back the registration if needed
			if let Some(guard) = guard{guard.set(instance_id.clone());	}
			Ok(RegisterResult::Stored(instance_id))
		} else {
			Ok(RegisterResult::AlreadyStored(instance_id))
		}
	}
}
