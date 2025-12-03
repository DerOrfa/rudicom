use crate::{db, tools};
use crate::tools::Error::DicomError;
use crate::tools::{entries_for_record, Result};
use dicom::encoding::TransferSyntaxIndex;
use dicom::object::{FileMetaTableBuilder, InMemDicomObject};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom_ul::pdu::{PDataValue, PDataValueType};
use std::io::{Cursor, Read, Write};
use dicom::object::mem::InMemElement;
use dicom_dictionary_std::tags;
use tracing::error;
use crate::db::Entry::{Instance, Series, Study};
use crate::db::RegisterResult;
use crate::dimse::definitions::{Status, StatusFailure};
use crate::dimse::definitions::StatusOk::Success;
use crate::dimse::message::to_dicom_err;
use crate::tools::error::DicomError::TransferSyntaxNotFound;

pub struct PDataReader<'a>{
	source: &'a mut tokio::sync::mpsc::UnboundedReceiver<PDataValue>,
	last: PDataValue,
}
impl<'a> PDataReader<'a> {
	pub(crate) fn new(source: &'a mut tokio::sync::mpsc::UnboundedReceiver<PDataValue>) -> Self {
		PDataReader{source,last:PDataValue{
			presentation_context_id:0,
			value_type:PDataValueType::Data,
			is_last: false,
			data: vec![],
		}}
	}
}

impl<'a> Read for PDataReader<'a> {
	fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
		if self.last.data.is_empty(){
			if self.last.is_last{return Ok(0)}
			match self.source.blocking_recv() {
				Some(v) => self.last = v,
				None => return Ok(0)
			}
		}
		let written = buf.write(&self.last.data)?;
		self.last.data.drain(.. written);
		Ok(written)
	}
}

pub async fn read_pipe(mut source:tokio::sync::mpsc::UnboundedReceiver<PDataValue>, prelude:Cursor<Vec<u8>>, ts:String)
					   -> Result<(InMemDicomObject, tokio::sync::mpsc::UnboundedReceiver<PDataValue>)>
{
	tokio::task::spawn_blocking(move || {
		let ts = TransferSyntaxRegistry.get(ts.as_str()).ok_or(TransferSyntaxNotFound(ts))?;

		let buffer = prelude.chain(PDataReader::new(&mut source));
		InMemDicomObject::read_dataset_with_ts(buffer, &ts)
			.map_err(|e|DicomError(e.into()))
			.map(|e|(e,source))
	}).await.expect("Reading thread panicked")
}

pub async fn store_db(obj:InMemDicomObject,ts:impl Into<String>) -> Status<()>
{
	let file_meta = FileMetaTableBuilder::new().transfer_syntax(ts);
	let obj = match obj.with_meta(file_meta){
		Ok(obj) => obj,
		Err(e) => {
			error!("Error {e} when preparing received data for storage");
			return StatusFailure::ProcessingFailure.into()
		}
	};
	match tools::store::store(obj).await
	{
		Ok(RegisterResult::Stored(id)) => Success(()).into(),
		Ok(RegisterResult::AlreadyStored(id)) => StatusFailure::DuplicateSOPInstance.into(),
		Err(e) => {
			error!("Error {e} when storing received data");
			StatusFailure::Failure.into()
		}
	}
}

/// Look up instances based on given identifier object
///
/// Extracts QUERY_RETRIEVE_LEVEL from the identifier to decide which UID field to extract to collect all instance Entries
/// - "PATIENT" -> not implemented yet
/// - "STUDY" -> STUDY_INSTANCE_UID
/// - "SERIES" -> SERIES_INSTANCE_UID
/// - "IMAGE" -> SOP_INSTANCE_UID
///
/// # returns
/// * list of found instance Entries
/// * empty list if nothing was found
/// * StatusFailure::InvalidArgument if QUERY_RETRIEVE_LEVEL is not one of the above or if expected UID fields from above cannot be extracted from identifier.

//https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.2.2.2.html
//https://dicom.nema.org/dicom/2013/output/chtml/part04/sect_C.4.html#sect_C.4.3.1.3.1
//https://dicom.nema.org/dicom/2013/Output/chtml/part04/sect_c.3.html @todo support "Composite Object Instance"
pub async fn lookup(ident: impl IntoIterator<Item = InMemElement>) -> Result<Vec<db::Entry>>
{
	let ident = InMemDicomObject::from_element_iter(ident);
	let level = ident.get(tags::QUERY_RETRIEVE_LEVEL)
		.ok_or(StatusFailure::InvalidArgument).map_err(|e|{error!("Query Retrieve Level is missing in identifier");e})?
		.to_str().map_err(to_dicom_err)?
		.to_uppercase();
	let (table,uids) = match level.as_str() {
		"PATIENT" => todo!(),
		"STUDY" => {
			let instances = ident.get(tags::STUDY_INSTANCE_UID)
				.ok_or(StatusFailure::InvalidArgument).map_err(|e|{error!("Expected Study Instance UID in identifier");e})?;
			("studies", instances)
		},
		"SERIES" => {
			let instances = ident.get(tags::SERIES_INSTANCE_UID)
				.ok_or(StatusFailure::InvalidArgument).map_err(|e|{error!("Expected Series Instance UID in identifier");e})?;
			("series", instances)
		},
		"IMAGE" => {
			let instances = ident.get(tags::SOP_INSTANCE_UID)
				.ok_or(StatusFailure::InvalidArgument).map_err(|e|{error!("Expected SOP Instance UID in identifier");e})?;
			("instances", instances)
		},
		_ => {
			error!("Invalid QueryRetrieveLevel {level}");
			Err(StatusFailure::InvalidArgument)?
		}
	};

	let mut ret = vec![];
	let uids:Vec<_> = uids.to_multi_str().map_err(to_dicom_err)?.iter().cloned().collect();
	for id in uids {
		if let Some(entry) = db::lookup_uid(table, id).await? {
			match entry {
				Instance(_) => ret.push(entry),
				Series((id, _)) | Study((id, _)) => {
					let mut entries = entries_for_record(&id, "instances").await?;
					ret.append(&mut entries);
				}
			}
		}
	}
	Ok(ret)
}
