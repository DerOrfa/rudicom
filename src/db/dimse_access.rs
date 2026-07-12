use crate::db::{lookup_uid, Entry, LocalSession, Session};
use crate::dcm::AttributeSelector;
use crate::tools::store::store_ob;
use crate::{db, tools};
use dicom::core::VR;
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dimse::definitions::FailureCode;
use dimse::identifier::Identifier;
use dimse::io::ItemResult;
use dimse::status::{failure, success, Comment, Offending, Status, StatusFailure};
use dimse::RetrieveLevel;
use futures::{stream, stream::BoxStream, StreamExt};
use std::collections::HashMap;
use std::path::PathBuf;
use surrealdb::types::ToSql;

#[derive(Clone)]
pub struct Accessor {}

impl dimse::io::FileAccess for Accessor {
	type Item = Entry;

	async fn get_uid(&self, item: &Self::Item) -> Result<String, StatusFailure> {
		Ok(item.id().key.to_sql())
	}

	async fn get_path(&self, item: &Self::Item) -> Result<PathBuf, StatusFailure> {
		item.get_path().await
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))
	}

	async fn store_file(&mut self, file: FileDicomObject<InMemDicomObject>) -> Status {
		store_ob(file, &mut LocalSession::create(&db::DB,1)).await
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))
			.map(|_| success().into())
	}

	async fn lookup<'a>(&self, ident: impl Into<Identifier> + Send) -> Result<BoxStream<'a, Self::Item>, StatusFailure> {
		let ident = ident.into();
		// gather stuff we need from ident
		let instance = ident.contains(tags::SOP_INSTANCE_UID).and_then(|e|e.to_str().ok());
		let series = ident.contains(tags::SERIES_INSTANCE_UID).and_then(|e|e.to_str().ok());
		let study = ident.contains(tags::STUDY_INSTANCE_UID).and_then(|e|e.to_str().ok());

		// the entry whose children we're looking for
		let lookup = if let Some(uid) = &instance {
			lookup_uid("instances",uid.to_string())
		} else if let Some(uid) = &series {
			lookup_uid("series",uid.to_string())
		} else if let Some(uid) = &study {
			lookup_uid("studies",uid.to_string())
		} else {
			return Err(failure(FailureCode::CannotUnderstand)
				.comment("Need at least one of SOPInstanceUID, SeriesInstanceUID or StudyInstanceUID"))
		};

		// figure out what table to look in
		let retrieve_table = match ident.level {
			Some(RetrieveLevel::IMAGE) => instance.map_or(Err(tags::SOP_INSTANCE_UID),|_|Ok("instances")),
			Some(RetrieveLevel::SERIES) => series.map_or(Err(tags::SERIES_INSTANCE_UID),|_|Ok("series")),
			Some(RetrieveLevel::STUDY) => study.map_or(Err(tags::STUDY_INSTANCE_UID),|_|Ok("studies")),
			Some(RetrieveLevel::PATIENT) => return Err(failure(FailureCode::InvalidArgument).comment("Cannot do patient level retrieve")),
			None => Err(tags::QUERY_RETRIEVE_LEVEL)
		}.map_err(|e|failure(FailureCode::MissingAttribute).offending([e]))?;

		// do the lookup
		let sel= lookup.await
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))?
			.ok_or_else(|| failure(FailureCode::NoSuchSOPInstance))?;
		let sel = tools::entries_for_record(sel.id(),retrieve_table).await
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))?;
		Ok(stream::iter(sel).boxed())
	}

	async fn find<'a>(&self, ident: impl Into<Identifier> + Send) -> Result<BoxStream<'a, ItemResult<InMemDicomObject>>, StatusFailure> {
		let ident = ident.into();
		// let tz_offset = ident.contains(tags::TIMEZONE_OFFSET_FROM_UTC)
		// 	.map(|e|e.to_str().map(Cow::into_owned)).transpose()
		// 	.map_err(|e|failure(FailureCode::InvalidArgument).offending([tags::TIMEZONE_OFFSET_FROM_UTC]).comment(e))?;

		let (table,known_db_tags) = match ident.level {
			Some(RetrieveLevel::IMAGE) => Ok(("instances",crate::config::get().instance_tags.clone())),
			Some(RetrieveLevel::SERIES) => Ok(("series",crate::config::get().series_tags.clone())),
			Some(RetrieveLevel::STUDY) => Ok(("studies",crate::config::get().study_tags.clone())),
			Some(RetrieveLevel::PATIENT) => return Err(failure(FailureCode::InvalidArgument).comment("Cannot do patient level find")),
			None => Err(tags::QUERY_RETRIEVE_LEVEL)
		}.map_err(|e|failure(FailureCode::MissingAttribute).offending([e]))?;

		let mut search_map:HashMap<_,_> = Default::default();
		for (db_key,dicom_attrs) in known_db_tags {
			for attr in dicom_attrs.into_iter()
				.filter_map(|a|if let AttributeSelector::Core(a)=a{Some(a)} else {None})
			{
				search_map.insert(attr.last_tag(),db_key.clone());
			}
		}

		let entries:Vec<_>= db::list_entries(table).await.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))?
			.into_iter().filter_map(|entry|{
				let mut matcher = InMemDicomObject::new_empty();
				for (tag, db) in &search_map {
					if let Some(found) = entry.get(db).and_then(|v|v.as_string()){
						matcher.put_str(tag.to_owned(),VR::LO,found);
					}
				}
				ident.matches_all(&matcher).then_some(Ok(matcher))
			}
		).collect();
		Ok(stream::iter(entries).boxed())

	}
}