use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;
use dicom_ul::pdu::PDataValue;
use std::path::PathBuf;
use crate::db;

/// Attachment for Payload can be a file reference, a DICOM object or empty
pub enum SendAttachment{
	File(db::File),
	Obj(InMemDicomObject),
	None
}

/// DIMSE payload to be sent to another peer
/// Consists of a DIMSE PDu data field that is a command and optionally an attachment
pub struct SendPayload{
	pub command:PDataValue,
	pub attachment:SendAttachment,
}

impl SendPayload {
	pub fn new(command:PDataValue, attach:impl IntoIterator<Item=InMemElement>) -> SendPayload {
		let attachment = attach.into_iter().collect::<Vec<_>>();
		let attachment = if attachment.len() == 0 {
			SendAttachment::None
		} else {
			SendAttachment::Obj(InMemDicomObject::from_element_iter(attachment))
		};
		SendPayload{command, attachment}
	}
}

impl From<PDataValue> for SendPayload {
	fn from(command:PDataValue) -> SendPayload {
		SendPayload::new(command, vec![])
	}
}

