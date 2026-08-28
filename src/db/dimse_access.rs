use crate::db::{DB, Entry, lookup_uid};
use crate::dcm::{AttributeSelector, INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::store::store_single_ob;
use crate::tools;
use dicom::core::header::Header;
use dicom::dictionary_std::tags;
use dicom::object::{DefaultDicomObject, FileDicomObject, InMemDicomObject, Tag};
use dimse::RetrieveLevel;
use dimse::definitions::FailureCode;
use dimse::identifier::Identifier;
use dimse::io::ItemResult;
use dimse::status::{Comment, Offending, Status, StatusFailure, failure, success};
use futures::{stream, stream::BoxStream, StreamExt, TryStreamExt};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use surrealdb::types as db_types;
use surrealdb::types::ToSql;
use tracing::debug;

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
		store_single_ob(file).await
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

		// use the QUERY_RETRIEVE_LEVEL to get the table and DB field->dicom tags mapping
		let (table, _id_tag,known_db_tags) = match ident.level {
			Some(RetrieveLevel::IMAGE) => Ok(("instances",tags::SOP_INSTANCE_UID,INSTANCE_TAGS.deref())),
			Some(RetrieveLevel::SERIES) => Ok(("series",tags::SERIES_INSTANCE_UID,SERIES_TAGS.deref())),
			Some(RetrieveLevel::STUDY) => Ok(("studies",tags::STUDY_INSTANCE_UID,STUDY_TAGS.deref())),
			Some(RetrieveLevel::PATIENT) => return Err(failure(FailureCode::InvalidArgument).comment("Cannot do patient level find")),
			None => Err(tags::QUERY_RETRIEVE_LEVEL)
		}.map_err(|e|failure(FailureCode::MissingAttribute).offending([e]))?;
		let mut table = table.to_string();
		// compute a dicom tag -> db field mapping from that (multiple dicom tags might have the same db field)
		let mut search_map:HashMap<_,_> = Default::default();
		for (db_key,dicom_attrs) in known_db_tags {
			for attr in dicom_attrs.into_iter()
				.filter_map(|a|if let AttributeSelector::Core(a)=a{Some(a)} else {None})
			{
				search_map.insert(attr.last_tag(),db_key.clone());
			}
		}
		// build a where clause for the lookup
		let mut whr = vec![];
		if ident.filters.is_empty(){
			Err(failure(FailureCode::MissingAttribute).comment("Search attribute is missing"))?
		}

		// remap filters to make them easier to deal with here
		let mut filters = ident.filters.into_iter().map(|f|
			f.to_str()
				.map_err(|e| failure(FailureCode::InvalidAttributeValue).offending([f.tag()]).comment(e))
					.map(|s|(f.tag(),s.to_string()))
		).collect::<Result<HashMap<Tag,String>,_>>()?;

		// try to reduce search area
		if let Some(instance) = filters.remove(&tags::SOP_INSTANCE_UID) {
			let instance = db_types::RecordId::new("instances",instance);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}",instance.to_sql()),
				_ => table
			}
		} else if let Some(series) = filters.remove(&tags::SERIES_INSTANCE_UID) {
			let series = db_types::RecordId::new("series",series);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}.instances", series.to_sql()),
				Some(RetrieveLevel::SERIES) => format!("{}", series.to_sql()),
				_ => table
			}
		} else if let Some(study) = filters.remove(&tags::STUDY_INSTANCE_UID){
			let study = db_types::RecordId::new("studies",study);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}.series.instances",study.to_sql()),
				Some(RetrieveLevel::SERIES) => format!("{}.series",study.to_sql()),
				Some(RetrieveLevel::STUDY) => format!("{}",study.to_sql()),
				_ => table
			}
		}

		for (tag, f) in filters {
			let filter_regex = f.replace('*', ".*"); // replace "*" with ".*" for regex

			// if a UID is given we can limit the search range ahead
			if filter_regex.is_empty() { continue; }
			else if let Some(d) = search_map.get(&tag){ // if we have what caller is looking for
				whr.push(format!("{d}.matches(\"{filter_regex}\")")); // collect parts of a where clause
			} else { // bail, let caller know we don't like his request
				Err(failure(FailureCode::NoSuchAttribute).offending([tag]))?;
			}
		}
		let query = if whr.is_empty() {
			format!("select * from {}", table)
		} else {
			format!("select * from {} where {}",table, whr.join(" and "))
		};
		let found:Vec<Entry> = DB.query(&query).await
			.and_then(|mut r|r.take(0))
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))?;
		debug!("Query \"{query}\" generated {} results", found.len());

		let stream = stream::iter(found)
			.then(|f|async move { f.get_files().await })
			.and_then(|f|async move {
				f.first().unwrap().read().await
			})
			.map(|o|o
				.map(DefaultDicomObject::into_inner)
				.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))
			);

		Ok(stream.boxed())
	}
}