use std::collections::{BTreeMap, HashMap};

use crate::db::{lookup, RecordId, RegisterResult, DB};
use crate::dcm::{INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::{extract_from_dicom, Error};
use crate::{dcm, tools};
use dcm::AttributeSelector;
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use surrealdb::types as db_types;
use surrealdb::types::{AlreadyExistsError, SurrealValue};

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
			let del = DB.delete::<Option<db_types::Value>>(id.0.clone());
			tokio::spawn(async { del.await });//todo https://github.com/tokio-rs/tokio/issues/2289
		}
		self.0=None;
	}
}

async fn insert<'a>(
	obj:&DefaultDicomObject,
	record_id: &RecordId,
	add_meta:Vec<(&'a str,db_types::Value)>,
	tags:&HashMap<String,Vec<AttributeSelector>>
) -> tools::Result<bool>
{
	let meta: BTreeMap<_, _> = 
		dcm::extract(&obj, &tags).into_iter()
		.chain([("uid", record_id.str_key().into_value())])
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect();

	// todo look into DB.begin()
	loop {
		match DB.insert::<Option<db_types::Value>>(&record_id.0).content(meta.clone()).await {
			Err(e) => {
				if e.kind_str() == "Transaction conflict"{
					tokio::time::sleep(tokio::time::Duration::from_millis(rand::random_range(10..100))).await;
					continue // try again
				} else if let Some(AlreadyExistsError::Record {id}) = e.already_exists_details(){
					if let Some(existing) = lookup(
						&RecordId(db_types::RecordId::new(record_id.table.to_owned(),id.to_owned())
					)).await?{
						return if existing == *obj {Ok(false)}
						else {Err(Error::DataConflict(existing))}
					} // gone again??, try to repeat insert
				} else {
					return Err(e.into())
				}
			},
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
	add_meta:Vec<(&'a str,db_types::Value)>,
	guard:Option<&mut RegistryGuard>
) -> tools::Result<RegisterResult> {

	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;

	let instance_id = RecordId::from_instance(instance_uid.as_ref());

	// try to avoid insert operation as much as possible
	if let Some(existing) = lookup(&instance_id).await?{
		if existing!=*obj {
			Err(Error::DataConflict(existing))
		} else { 
			Ok(RegisterResult::AlreadyStored(instance_id)) 
		}
	} else {
		let series_id= RecordId::from_series(series_uid.as_ref());
		if let Some(existing) = lookup(&series_id).await? {
			if existing!=*obj { return Err(Error::DataConflict(existing)) }
		} else {
			let study_id = RecordId::from_study(study_uid.as_ref());
			if let Some(existing) = lookup(&study_id).await? {
				if existing!=*obj { return Err(Error::DataConflict(existing)) }
			} else {
				insert(obj, &study_id, vec![], &STUDY_TAGS).await?;
			}
			insert(obj, &series_id, vec![("study",study_id.0.into_value())], &SERIES_TAGS).await?;
		}
		if insert(obj, &instance_id,
			add_meta.into_iter().chain([("series",series_id.0.into_value())]).collect(),
			&INSTANCE_TAGS
		).await?{
			// successfully inserted
			// set up the guard with the registered instance, so we can roll back the registration if needed
			if let Some(guard) = guard{guard.set(instance_id.clone());	}
			Ok(RegisterResult::Stored(instance_id))
		} else {
			Ok(RegisterResult::AlreadyStored(instance_id))
		}
	}
}
