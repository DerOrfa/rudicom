use crate::{db, tools};
use crate::db::{lookup_uid, Entry};
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dimse::definitions::FailureCode;
use dimse::identifier::Identifier;
use dimse::io::ItemResult;
use dimse::status::{failure, success, Comment, Offending, Status, StatusFailure};
use dimse::RetrieveLevel;
use futures::{StreamExt,stream, stream::BoxStream};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Accessor {}

impl dimse::io::FileAccess for Accessor {
	type Item = Entry;

	async fn get_uid(&self, item: &Self::Item) -> Result<String, StatusFailure> {
		Ok(item.id().key().to_string())
	}

	async fn get_path(&self, item: &Self::Item) -> Result<PathBuf, StatusFailure> {
		item.get_path().await
			.map_err(|e|failure(FailureCode::ProcessingFailure).comment(e))
	}

	async fn store_file(&mut self, file: FileDicomObject<InMemDicomObject>) -> Status {
		db::register_instance(&file,vec![],None).await
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
		todo!()
	}
}