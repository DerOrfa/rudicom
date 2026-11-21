use crate::db::{Entry, Pickable, DB, RecordId};
use crate::dcm::{AttributeSelector, INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};
use crate::tools::store::store_single_ob;
use crate::tools;
use dicom::core::header::Header;
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject, Tag};
use dimse::RetrieveLevel;
use dimse::definitions::FailureCode;
use dimse::identifier::Identifier;
use dimse::io::ItemResult;
use dimse::status::{Comment, Offending, Status, StatusFailure, failure, success};
use futures::{stream, stream::BoxStream, StreamExt};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::PathBuf;
use dicom::core::VR;
use dicom::dictionary_std::tags::{NUMBER_OF_SERIES_RELATED_INSTANCES, NUMBER_OF_STUDY_RELATED_INSTANCES, NUMBER_OF_STUDY_RELATED_SERIES};
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

	async fn lookup_instances<'a>(&self, ident: impl Into<Identifier> + Send) -> Result<BoxStream<'a, Self::Item>, StatusFailure> {
		let ident = ident.into();
		let get_record = |tag|
			ident.contains(tag).and_then(|e|e.to_str().ok()).ok_or(tag);

		// figure out what table to look in
		let parent = match ident.level {
			Some(RetrieveLevel::IMAGE) =>
				get_record(tags::SOP_INSTANCE_UID).map(|id|RecordId::from_instance(id.as_ref())),
			Some(RetrieveLevel::SERIES) =>
				get_record(tags::SERIES_INSTANCE_UID).map(|id|RecordId::from_series(id.as_ref())),
			Some(RetrieveLevel::STUDY) =>
				get_record(tags::STUDY_INSTANCE_UID).map(|id|RecordId::from_study(id.as_ref())),
			Some(RetrieveLevel::PATIENT) => return Err(failure(FailureCode::InvalidArgument).comment("Cannot do patient level retrieve")),
			None => Err(tags::QUERY_RETRIEVE_LEVEL)
		}.map_err(|e|failure(FailureCode::MissingAttribute).offending([e]))?;

		// do the lookup
		let sel = tools::entries_for_record(&parent,"instances").await
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
					.map(|s|(f.tag(),(s.to_string(),f.vr())))
		).collect::<Result<HashMap<Tag,_>,_>>()?;

		// try to reduce search area
		if let Some((instance,_)) = filters.remove(&tags::SOP_INSTANCE_UID) {
			let instance = db_types::RecordId::new("instances",instance);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}",instance.to_sql()),
				_ => table
			}
		} else if let Some((series,_)) = filters.remove(&tags::SERIES_INSTANCE_UID) {
			let series = db_types::RecordId::new("series",series);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}.instances", series.to_sql()),
				Some(RetrieveLevel::SERIES) => format!("{}", series.to_sql()),
				_ => table
			}
		} else if let Some((study,_)) = filters.remove(&tags::STUDY_INSTANCE_UID){
			let study = db_types::RecordId::new("studies",study);
			table = match ident.level {
				Some(RetrieveLevel::IMAGE) => format!("{}.series.instances",study.to_sql()),
				Some(RetrieveLevel::SERIES) => format!("{}.series",study.to_sql()),
				Some(RetrieveLevel::STUDY) => format!("{}",study.to_sql()),
				_ => table
			}
		}

		for (tag, (f,vr)) in filters {
			if f.is_empty() { continue; }
			else if let Some(db_field) = search_map.get(&tag){ // if we have what caller is looking for
				match vr { // collect parts of a where clause
					// if it's a date string or range
					VR::DA if !f.contains(|c:char|!(c.is_ascii_digit() || c == '-') ) => {
						if let Some(split)= f.find('-') { // it's a range
							let (start,mut end) = f.split_at(split);
							end = end.get(1..).expect("there should be a '-'");
							// add first part
							whr.push(format!("fn::date_from_str({db_field}) >= fn::date_from_str('{start}')"));
							if !end.is_empty() { // if there
								whr.push(format!("fn::date_from_str({db_field}) < fn::date_from_str('{end}')"));
							}
						} else { // exact date
							whr.push(format!("{db_field}) == {f}"));
						}
					}
					_ => {
						let filter_regex = f.replace('*', ".*"); // replace "*" with ".*" for regex
						// if a UID is given we can limit the search range ahead
						whr.push(format!("{db_field}.matches(\"{filter_regex}\")"));
					}
				}
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
			.then(move|e| process_entry(e,ident.level.clone()))
			.map(|o|o
				.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))
			);

		Ok(stream.boxed())
	}
}

async fn process_entry(entry: Entry, level: Option<RetrieveLevel>) -> tools::Result<InMemDicomObject> {
	let files = entry.get_files().await?;
	let mut obj = files.first().unwrap().read().await?;

	if let Some(level) = level {
		match level {
			RetrieveLevel::SERIES => {
				obj.put_str(NUMBER_OF_SERIES_RELATED_INSTANCES, VR::IS, files.len().to_string());
			},
			RetrieveLevel::STUDY => {
				obj.put_str(NUMBER_OF_STUDY_RELATED_INSTANCES, VR::IS, files.len().to_string());
				if let Some(ser) = entry.pick_ref("series").ok().and_then(|v|v.as_array()) {
					obj.put_str(NUMBER_OF_STUDY_RELATED_SERIES, VR::IS, ser.len().to_string());
				}
			},
			_ => {},
		};
	}
	Ok(obj.into_inner())
}