use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use crate::db::{if_retry, Entry, File, RecordId, RegisterResult, Session};
use crate::dcm::{INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::{extract_from_dicom, Error};
use crate::{dcm, tools};
use dcm::AttributeSelector;
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use itertools::Itertools;
use surrealdb::method::Transaction;
use surrealdb::{types as db_types, Connection};
use surrealdb::engine::any::Any;
use surrealdb::types::{SurrealValue, ToSql};
use tracing::{debug};
use crate::tools::Error::{DataConflict, FieldConflict};

#[derive(Default,Debug,Clone,SurrealValue)]
struct Diff
{
	op:String,
	path:String,
	value:db_types::Value,
}

pub enum FileInfo{
	Exists(File),
	Stored(Option<File>),
	Store
}

impl FileInfo{
	pub fn commit(&mut self){
		if let FileInfo::Stored(s) = self {
			s.take();
		} else {
			debug!("commiting non-stored file");
		}
	}
}
impl Drop for FileInfo{
	fn drop(&mut self) {
		if let FileInfo::Stored(s) = self {
			if let Some(f) = s.take() {
				debug!("dropping uncommited file {}",f.get_path().display());
				tokio::spawn(f.remove());
			}
		}
	}
}
fn prepare_content<'a>(
	obj:&DefaultDicomObject,
	add_meta:impl IntoIterator<Item=(&'a str, db_types::Value)>,
	tags:&'a HashMap<String, Vec<AttributeSelector>>
) -> impl SurrealValue
{
	dcm::extract(&obj, &tags).into_iter()
		.chain(add_meta)
		.map(|(k,v)| (k.to_string(), v))
		.collect::<BTreeMap<_, _>>()
}

async fn insert<'a,C>(
	obj:&DefaultDicomObject,
	record_id: &RecordId,
	add_meta:impl IntoIterator<Item=(&'a str,db_types::Value)>,
	tags:&'a HashMap<String, Vec<AttributeSelector>>,
	transaction: &Transaction<C>
) -> tools::Result<bool> where C:Connection
{
	let meta= prepare_content(obj, add_meta, tags).into_value();
	// use UPSERT so we can get a BEFORE to compare it if data existed, the whole transaction will
	// be canceled anyway, so there is no harm overwriting
	let q = transaction.query("UPSERT ONLY $rec CONTENT $content RETURN BEFORE")
		.bind(("content",meta)).bind(("rec",record_id.0.clone()));

	if let Some(existing) = q.await?
		.take::<Option<db_types::Value>>(0)?
		.map(Entry::try_from).transpose()?
	{
		if existing == *obj {Ok(false)} // @todo should not compare file as this validly might not be there yet
		else {Err(DataConflict(existing))}
	} else {
		Ok(true)
	}
}

async fn upsert<'a,C>(
	obj:&DefaultDicomObject,
	record_id: &RecordId,
	add_meta:Vec<(&'a str,db_types::Value)>,
	tags:&HashMap<String,Vec<AttributeSelector>>,
	transaction: &Transaction<C>
) -> tools::Result<bool> where C:Connection
{
	let meta= prepare_content(obj, add_meta, tags);
	let q = transaction.query("UPSERT ONLY $rec MERGE $content RETURN diff")
		.bind(("content",meta)).bind(("rec",record_id.0.clone()));
	let diff  = q.await?.take::<Vec<Diff>>(0)?.into_iter()
		.filter(|d|d.op!="add")
		.filter(|d|!d.path.starts_with("/instances")).filter(|d|!d.path.starts_with("/series"))
		.collect::<Vec<_>>();
	if diff.is_empty(){
		Ok(true)
	} else {
		debug!("Field conflicts in {}:\n{}", record_id, diff.clone().into_value().to_sql_pretty());
		Err(FieldConflict{ fields: diff.into_iter().map(|d|format!("{}",d.path)).join(":"), id: record_id.clone() })
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
pub async fn register_instance<S>(
	obj:impl Into<Arc<DefaultDicomObject>>,
	file_info:&mut FileInfo,
	session: &mut S,
) -> tools::Result<RegisterResult> where S:Session<Any>
{
	let mut res = None;
	let mut retry = 0;
	let mut transaction = None;
	let obj = obj.into();
	loop {
		// make sure we have a transaction
		let t= match &mut transaction {
			Some(t) => t,
			None => {
				transaction = Some(session.begin().await?);
				transaction.as_mut().unwrap()
			}
		};

		// make sure we have a result and handle it
		let fall_throu = match if let Some(r) = res.take() {r} // we already have a result, don't need a new one
			else {
				retry+=1;
				_register_instance(obj.clone(), file_info, t).await
		}{
			Err(Error::SurrealError(e)) =>
				if let Ok(true) = if_retry(&e,&mut retry).await{continue} else { e.into() },
			Err(e) => e,
			Ok(r) => { // no inner errors, commit or cancel based on results
					let commit_or_cancel = match r {
						RegisterResult::Stored(_) => // normal store, commit the store, and then the file
							transaction.take().unwrap().commit().await
								.map(|_|{file_info.commit();} ),
						RegisterResult::AlreadyStored(_) =>
							transaction.take().unwrap().cancel().await,
					};
					match commit_or_cancel { // results of the commit/reset above
						Ok(_) => return Ok(r), // all good, we're done
						Err(e) =>{
							res = Some(Err(e.into())); //darn, whatever the error is, stuff it back in, and handle it in the next loop
							continue
						}
					}
				},
		};
		// should only be here because of a final error, cancel transaction and get out of here
		if let Some(t) = transaction {
			t.cancel().await?;
		}
		return Err(fall_throu)
	}

}

async fn _register_instance<'a,C>(
	obj:Arc<DefaultDicomObject>,
	fileinfo:&mut FileInfo,
	transaction: &Transaction<C>
) -> tools::Result<RegisterResult> where C:Connection
{
	let study_uid = extract_from_dicom(&*obj, tags::STUDY_INSTANCE_UID)?;
	let series_uid = extract_from_dicom(&*obj, tags::SERIES_INSTANCE_UID)?;
	let instance_uid = extract_from_dicom(&*obj, tags::SOP_INSTANCE_UID)?;

	let study_id = RecordId::from_study(study_uid.as_ref());
	let series_id= RecordId::from_series(series_uid.as_ref());
	let instance_id = RecordId::from_instance(instance_uid.as_ref());
	let mut add_meta = vec![("series",series_id.0.to_owned().into_value())];

	match fileinfo{
		FileInfo::Exists(file)|FileInfo::Stored(Some(file)) => {
			add_meta.push(("file", file.clone().try_into()?));
		}
		_ =>{}
	};
	debug!("registering instance {}",instance_uid);

	if insert(&*obj, &instance_id, add_meta, &INSTANCE_TAGS, &transaction	).await?
	{ // normal insert, didn't exist before. So make sure its series/study exists (this may also update non-existing entries)
		upsert(&*obj, &series_id, vec![("study", study_id.0.clone().into_value())], &SERIES_TAGS, &transaction).await?;
		upsert(&*obj, &study_id, vec![], &STUDY_TAGS, &transaction).await?;

		// everything successfully inserted
		// now do the file, if it's not there yet
		if let FileInfo::Store = fileinfo{
			let file = File::new_from_obj(obj).await?; // storing failed, abort
			*fileinfo = FileInfo::Stored(Some(file.clone()));
			transaction.query("UPDATE $rec SET file = $file")
				.bind(("rec",instance_id.0.clone()))
				.bind(("file", db_types::Value::try_from(file)?))
				.await?;
		}

		// set up the guard with the registered instance, so we can roll back the registration if needed
		// @todo we could return the transaction so the caller could cancel (or just drop) it, no guard needed
		Ok(RegisterResult::Stored(instance_id))

	} else { // didn't insert as it's already there. But it's exactly the same, so all good
		Ok(RegisterResult::AlreadyStored(instance_id))
	}

}
