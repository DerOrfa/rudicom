use std::future::Future;
use std::path::PathBuf;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dimse::identifier::Identifier;
use dimse::io::ItemResult;
use dimse::status::{Status, StatusFailure};
use futures::stream::BoxStream;
use crate::db::Entry;

#[derive(Clone)]
pub struct Accessor {}

impl dimse::io::FileAccess for Accessor {
	type Item = Entry;

	async fn get_uid(&self, uid: &Self::Item) -> Result<String, StatusFailure> {
		todo!()
	}

	async fn get_path(&self, uid: &Self::Item) -> Result<PathBuf, StatusFailure> {
		todo!()
	}

	async fn store_file(&mut self, file: FileDicomObject<InMemDicomObject>) -> Status {
		todo!()
	}

	async fn lookup<'a>(&self, ident: impl Into<Identifier> + Send) -> Result<BoxStream<'a, Self::Item>, StatusFailure> {
		todo!()
	}

	async fn find<'a>(&self, ident: impl Into<Identifier> + Send) -> Result<BoxStream<'a, ItemResult<InMemDicomObject>>, StatusFailure> {
		todo!()
	}
}