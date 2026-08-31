use std::collections::{BTreeMap, HashMap};
use crate::db::{if_retry, Entry, RecordId, RegisterResult, Session, register_manager, lookup};
use crate::dcm::{INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::{extract_from_dicom, Error, Context};
use crate::{dcm, tools};
use dcm::AttributeSelector;
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use itertools::Itertools;
use surrealdb::method::Transaction;
use surrealdb::{types as db_types, Connection};
use surrealdb::types::{SurrealValue, ToSql};
use tracing::{debug, error};
use crate::db::RegisterResult::AlreadyStored;
use crate::tools::Error::{DataConflict, FieldConflict, IdNotFound, SurrealError};

#[derive(Default,Debug,Clone,SurrealValue)]
struct Diff
{
	op:String,
	path:String,
	value:db_types::Value,
}
fn extract_record_ids(obj: &DefaultDicomObject) -> tools::Result<(RecordId, RecordId, RecordId)>
{
	let study_uid = extract_from_dicom(obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(obj, tags::SOP_INSTANCE_UID)?;

	Ok((
		RecordId::from_instance(instance_uid.as_ref()),
		RecordId::from_series(series_uid.as_ref()),
		RecordId::from_study(study_uid.as_ref()),
	))
}
pub(crate) fn prepare_content<'a>(
	obj:&DefaultDicomObject,
	add_meta:impl IntoIterator<Item=(&'a str, db_types::Value)>,
	tags:&'a HashMap<String, Vec<AttributeSelector>>
) -> BTreeMap<String, db_types::Value>
{
	dcm::extract(obj, &tags).into_iter()
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect()
}

async fn insert<'a,C>(
	obj:&DefaultDicomObject,
	record_id: &RecordId,
	add_meta:impl IntoIterator<Item=(&'a str,db_types::Value)>,
	tags:&'a HashMap<String, Vec<AttributeSelector>>,
	transaction: &Transaction<C>
) -> tools::Result<RegisterResult> where C:Connection
{
	let meta= prepare_content(obj, add_meta, tags).into_value();
	// use UPSERT so we can get a BEFORE to compare it if data existed, the whole transaction will
	// be canceled anyway, so there is no harm overwriting
	let q = transaction.query("UPSERT ONLY $rec CONTENT $content RETURN BEFORE")
		.bind(("content",meta)).bind(("rec",record_id.0.clone()));

	if let Some(existing) = q.await?
		.take::<Option<Entry>>(0)?
	{
		if existing == *obj {
			Ok(AlreadyStored(record_id.clone()))
		} else {
			Err(DataConflict(existing))
		}
	} else {
		Ok(RegisterResult::Stored(record_id.clone()))
	}
}

/// Runs insert on all images and then an upsert on the series and study entry.
///
/// The study and series data a generated from the first image.
/// The user must guarantee that all images would generate the same data.
///
/// Early fails will send the error back to their respective receivers and the images are removed
/// from the list (and thus the file if uncommited).
///
/// A single transaction is started from `session` and either commited (returns Ok), or canceled (returns Err).
/// A failed transaction does *not* remove images from the list, they can be used on the retry.
pub async fn queued_insert<S,C>(
	images:&mut Vec<register_manager::QEntry>,
	session: &mut S
) -> tools::Result<()> where S:Session<C>, C:Connection
{
	let mut retry = 0;
	while let Some(first) = images.first() {
		let (_, series_id, study_id) = extract_record_ids(first.image.as_ref())?;
		// go through all instances and insert them
		let transaction = session.begin().await?;
		let mut idx = 0;
		while idx < images.len() {
			let instance_id = RecordId::from_instance(extract_from_dicom(images[idx].image.as_ref(), tags::SOP_INSTANCE_UID)?.as_ref());

			let add_meta = vec![
				("series",series_id.clone().0.into_value()),
				("file", images[idx].image.get_fileinfo().expect("Image should have fileinfo").try_into()?),
			];

			match insert(images[idx].image.as_ref(), &instance_id, add_meta, &INSTANCE_TAGS, &transaction).await {
				// keep those that succeeded (not including already existing entries)
				Ok(RegisterResult::Stored(r)) => {
					images[idx].register_result=Some(RegisterResult::Stored(r));
					idx+=1
				},
				Ok(AlreadyStored(r)) => {
					let entry = images.remove(idx);
					let my_md5 = entry.image.get_md5().expect("Image should be saved and should have a checksum");
					let existing_md5 = lookup(&r).await?
						.ok_or(IdNotFound {id:r.to_string()}).context("When looking for a supposedly already existing entry")?
						.get_file()?.get_md5().to_string();
					if let Err(e) = entry.tx.send(
					if existing_md5 != my_md5 {
							Err(Error::Md5Conflict {
								existing_md5:existing_md5.to_string(),
								existing_id:r.clone(),
								my_md5:my_md5.to_string(),
							})
						} else { Ok(AlreadyStored(r)) }
					) { // log error if that fails
						error!("failed to let receiver know about failed insert ({})",
						e.map(|_|"already exists".to_string()).unwrap_or_else(|e|e.to_string()));
					};
				},
				// everything else can already be dropped, just let the receiver know
				Err(e) => if let Err(e) = images.remove(idx).tx.send(Err(e)) { // log error if that fails
					error!("failed to let receiver know about failed insert ({})",e.err().unwrap());
				}
			}
		}

		// if at least one was inserted, do study and series as well
		if !images.is_empty() { // @would be useful to let the caller know that the whole set failed
			let ser = upsert(images[0].image.as_ref(), &series_id, vec![("study", study_id.0.clone().into_value())], &SERIES_TAGS, &transaction).await;
			let std = upsert(images[0].image.as_ref(), &study_id, vec![], &STUDY_TAGS, &transaction).await;
			if ser.is_err() || std.is_err() { //the series or the study update failed, that means we can throw away the whole set
				let e = ser.and(std).err().unwrap();
				images.drain(..).for_each(|entry| { // let all receivers know
					let e_cloned= match &e { // some errors can't be cloned, luckily the relevant ones can
						SurrealError(e) => e.clone().into(),
						FieldConflict { fields, id }
							=> FieldConflict{fields:fields.clone(), id:id.clone()},
						_ => unreachable!()
					};
					if let Err(e) = entry.tx.send(Err(e_cloned)) { // log error if that fails
						error!("failed to let receiver know about failed insert ({})",e.err().unwrap());
					}
				});
				return Err(e.into());
			}
		}
		// do commit and possibly try again if error was just write conflict
		return match transaction.commit().await {
			Err(e) => if let Ok(true) = if_retry(&e, &mut retry).await {// retry maybe
				retry += 1;
				continue
			} else { Err(e) },
			result => result
		}.map_err(|e|e.into())
	}
	Ok(())
}

async fn upsert<'a,C>(
	obj:&DefaultDicomObject,
	record_id: &RecordId,
	add_meta:Vec<(&'a str,db_types::Value)>,
	tags:&HashMap<String,Vec<AttributeSelector>>,
	transaction: &Transaction<C>
) -> tools::Result<()> where C:Connection
{
	let meta= prepare_content(obj, add_meta, tags);
	upsert_meta(meta,record_id,transaction).await
}
async fn upsert_meta<'a,C>(
	meta:BTreeMap<String,db_types::Value>,
	record_id: &RecordId,
	transaction: &Transaction<C>
) -> tools::Result<()> where C:Connection
{
	let q = transaction.query("UPSERT ONLY $rec MERGE $content RETURN diff")
		.bind(("content",meta)).bind(("rec",record_id.0.clone()));
	let diff  = q.await?.take::<Vec<Diff>>(0)?.into_iter()
		.filter(|d|d.op!="add")
		.filter(|d|!d.path.starts_with("/instances")).filter(|d|!d.path.starts_with("/series"))
		.collect::<Vec<_>>();
	if diff.is_empty(){
		Ok(())
	} else {
		debug!("Field conflicts in {}:\n{}", record_id, diff.clone().into_value().to_sql_pretty());
		Err(FieldConflict{ fields: diff.into_iter().map(|d|format!("{}",d.path)).join(":"), id: record_id.clone() })
	}
}
