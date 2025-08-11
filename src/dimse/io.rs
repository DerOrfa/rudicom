use std::future::Future;
use crate::{db, tools};
use crate::tools::error::DicomError::DicomTransferSyntaxNotFound;
use crate::tools::Error::DicomError;
use crate::tools::{lookup_instance_file, Result};
use dicom::encoding::TransferSyntaxIndex;
use dicom::object::{FileMetaTableBuilder, InMemDicomObject};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom_ul::pdu::{PDataValue, PDataValueType};
use std::io::{Cursor, Read, Write};
use std::sync::Arc;
use dicom_ul::ServerAssociation;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

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
		let ts = TransferSyntaxRegistry.get(ts.as_str()).ok_or(DicomTransferSyntaxNotFound(ts))?;

		let buffer = prelude.chain(PDataReader::new(&mut source));
		InMemDicomObject::read_dataset_with_ts(buffer, &ts)
			.map_err(|e|DicomError(e.into()))
			.map(|e|(e,source))
	}).await.expect("Reading thread panicked")
}

pub async fn store_db(obj:InMemDicomObject,ts:impl Into<String>) -> Result<()>
{
	let file_meta = FileMetaTableBuilder::new().transfer_syntax(ts);
	let obj = obj.with_meta(file_meta)
		.map_err(|e|DicomError(e.into()))?;
	tools::store::store(obj).await?;
	Ok(())
}
