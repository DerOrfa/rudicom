use std::borrow::Cow;
use dicom_ul::pdu::{PDataValue, PDataValueType};
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use dicom::encoding::{TransferSyntaxIndex};
use dicom::object::{AccessError, InMemDicomObject, OpenFileOptions};
use dicom::transfer_syntax::entries::IMPLICIT_VR_LITTLE_ENDIAN;
use tracing::{debug, error, warn};
use dicom::core::{DataElement, Tag, VR};
use dicom::core::dictionary::UidDictionary;
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::transfer_syntax::TransferSyntaxRegistry;
use tokio::task::JoinHandle;
use crate::db::File;
use crate::dimse::io;
use crate::dimse::payload::{SendAttachment, SendPayload};
use crate::tools::error::DicomError::DicomTransferSyntaxNotFound;
use crate::tools::{Context, Result};
use super::io::{store_db};
use super::definitions::*;

// https://dicom.nema.org/medical/dicom/current/output/chtml/part04/chapter_Z.html
// https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.6.html
// https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.3.4.html
// https://dicom.nema.org/medical/dicom/current/output/chtml/part08/PS3.8.html
// https://dicom.nema.org/dicom/2013/output/chtml/part07/sect_9.3.html

pub fn to_dicom_err(e:impl Into<crate::tools::error::DicomError>) -> crate::tools::Error
{
	crate::tools::Error::DicomError(e.into())
}
pub struct Message
{
	id:Arc<(AtomicU16,AtomicBool)>,
	handle:JoinHandle<Result<()>>,
	pub to_task: tokio::sync::mpsc::UnboundedSender<PDataValue>,
}

#[derive(Debug)]
pub struct Command{
	pub id:u16,
	pub status:Option<u16>,
	pub pc_id:u8,
	pub obj:InMemDicomObject,
	pub succeeded:u16,
	pub warn:u16,
	pub fail:u16,
	pub to_do:u16,
}

// https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_C.html
#[derive(Default,Debug)]
enum OpStatus{
	Success,
	Pending,
	Cancelled, // FE00H
	Warning,
	AttrWarning{affected_sop_class:String,affected_instance:String,offending:Vec<Tag>}, // 0107H,
	AttrValueOutOfRange(Vec<String>), // 0116H
	#[default]
	Failure,
	Refused{cause:String,offending:Vec<Tag>},
	DuplicateInstance(String), // 0111H
	InvalidInstance(String), //0117H
}

#[derive(Debug)]
struct Reply{
	status:Status<InMemDicomObject>,
	succeeded:Option<u16>,
	failed:Option<u16>,
	warning:Option<u16>
}

impl Command {
	pub fn sop_class(&self) -> std::result::Result<&InMemElement, AccessError> { self.obj.element(tags::AFFECTED_SOP_CLASS_UID) }
	pub fn instance_uid(&self) -> std::result::Result<&InMemElement, AccessError> { self.obj.element(tags::AFFECTED_SOP_CLASS_UID) }
	pub fn msgid(&self) -> std::result::Result<&InMemElement, AccessError> { self.obj.element(tags::MESSAGE_ID) }
	pub fn rspid(&self) -> std::result::Result<&InMemElement, AccessError> { self.obj.element(tags::MESSAGE_ID_BEING_RESPONDED_TO) }
	fn send_completed_subop<T>(&mut self, task: &mut MessageTask, status: impl Into<Status<T>>) -> Result<()> {
		self.to_do -=1;
		let status = status.into();
		match status {
			Ok(StatusOk::Success(_)) | Ok(StatusOk::Pending(_)) => self.succeeded+=1,
			Ok(StatusOk::Warning(_)) => self.warn+=1,
			Err(_) => self.fail+=1
		}
		let attr = [
			DataElement::new(tags::NUMBER_OF_COMPLETED_SUBOPERATIONS, VR::US, dicom_value!(self.succeeded)),
			DataElement::new(tags::NUMBER_OF_FAILED_SUBOPERATIONS, VR::US, dicom_value!(self.fail)),
			DataElement::new(tags::NUMBER_OF_WARNING_SUBOPERATIONS, VR::US, dicom_value!(self.warn)),
			DataElement::new(tags::NUMBER_OF_REMAINING_SUBOPERATIONS, VR::US, dicom_value!(self.to_do)),
		];
		let resp = self.make_response(task, status, attr, vec![]).map_err(to_dicom_err)?;
		task.sink.send(resp).map_err(|e| e.into())
	}
	fn make_response<T>(&self, task: &mut MessageTask, status: impl Into<Status<T>>, attr: impl IntoIterator<Item=InMemElement>, identifier: impl IntoIterator<Item=InMemElement>)
		-> std::result::Result<SendPayload, dicom::object::WriteError> {
		let identifier: Vec<_> = identifier.into_iter().collect();
		let identifier = if identifier.is_empty() {
			SendAttachment::None
		} else {
			SendAttachment::Obj(InMemDicomObject::from_element_iter(identifier))
		};
		let attr = attr.into_iter().chain([
			DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [0x8000 | self.id])),
			DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [get_status(status)]))
		]);
		let mut ret = InMemDicomObject::command_from_element_iter(attr);
		if let Some(msgid) = self.msgid().ok() {
			ret.put(DataElement::new(tags::MESSAGE_ID_BEING_RESPONDED_TO, VR::US, msgid.value().clone()));
		}
		if let Some(sop_class) = self.sop_class().ok() { ret.put(sop_class.clone()); }
		if let Some(sop_instance) = self.instance_uid().ok() { ret.put(sop_instance.clone()); }
		make_command(self.pc_id, ret, identifier)
	}
	fn respond<T>(&self, res: Status<T>, task: &mut MessageTask, attr: impl IntoIterator<Item=InMemElement>)
		-> Result<()> where T:IntoIterator<Item=InMemElement>
	{
		match res
		{
			Ok(StatusOk::Warning(w)) => {}
			Ok(StatusOk::Success(attachment)) | Ok(StatusOk::Pending(attachment)) => { // result to be sent is ok
				match self.make_response(task, StatusOk::Success(()), attr, attachment) {
					Ok(payload) => task.sink.send(payload)?,
					Err(e) => { // Making response failed. Will tell receiver something went wrong and return error
						let payload = self.make_response::<T>(task, StatusFailure::Failure, vec![], vec![]).expect("making an error response shouldn't fail");
						task.sink.send(payload)?;
						return Err(to_dicom_err(e).context("when building response"))
					}
				}
			}
			Err(e) => {
				// if error is a "dimse-error" send that, otherwise just "Fail"
				let payload = self.make_response::<T>(task, e, vec![], vec![]).expect("making an error response shouldn't fail");
				task.sink.send(payload)?;
			}
		}
		Ok(())
	}
}
struct MessageTask{
	id: Arc<(AtomicU16, AtomicBool)>,
	pc_id:u8,
	selected_ts:String,
	source:Option<tokio::sync::mpsc::UnboundedReceiver<PDataValue>>,
	sink: tokio::sync::mpsc::UnboundedSender<SendPayload>,
}
impl MessageTask
{
	async fn run(
		data: Vec<PDataValue>,
		id:Arc<(AtomicU16,AtomicBool)>,
		selected_ts: String,
		source:tokio::sync::mpsc::UnboundedReceiver<PDataValue>,
		sink: tokio::sync::mpsc::UnboundedSender<SendPayload>,
	) -> Result<()> {
		assert!(!data.is_empty());
		assert_eq!(data[0].value_type,PDataValueType::Command);
		let pc_id = data[0].presentation_context_id;
		let mut task = MessageTask{id, pc_id, selected_ts,source:Some(source),sink};
		let mut cmd = task.read_command(data).await?;

		let id= cmd.msgid().ok().map(InMemElement::to_int).transpose().map_err(to_dicom_err)?;
		id.map(|id|task.set_id(id));

		match cmd.id {
			C_STORE_RQ => { //C-STORE-RQ
				debug!("processing store request {}", id.map(|i|i.to_string()).unwrap_or("NO_ID".to_string()));
				let stored = store_db(task.fetch_obj(vec![],None).await?,task.selected_ts.as_str()).await
					.map(|s|StatusOk::<Vec<InMemElement>>::from(s));
				cmd.respond(stored,&mut task,vec![])
			}
			C_GET_RQ => {
				// https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.4.3.html
				// https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_9.html#sect_9.1.3.2
				// https://dicom.nema.org/medical/dicom/current/output/chtml/part04/chapter_Z.html
				let ident = task.fetch_obj(vec![], None).await?;
				let instances = io::lookup(ident).await?;
				debug!("Got GET request for {} instance(s)", instances.len());
				let mut failed = vec![];
				if instances.is_empty() {
					cmd.send_completed_subop::<()>(&mut task, StatusFailure::NoSuchSOPInstance)?;
				} else {
					cmd.to_do = instances.len() as u16;
					for instance in instances {
						let file = instance.get_file()?;
						let res = if file.get_path().exists(){
							task.c_store(file, vec![]).await.map_err(|e|e.to_string())
						} else {
							Err(format!("File {} does not exist for instance {}", file.get_path().display(), instance.id().str_key()))
						};
						match res {
							Ok(_) => cmd.send_completed_subop::<()>(&mut task,StatusOk::Success(()))?,
							Err(e) => {
								cmd.send_completed_subop::<()>(&mut task,StatusFailure::ProcessingFailure)?;
								failed.push(instance);
								error!(e);
							}
						}
					}
				}
				if !failed.is_empty() {
					cmd.respond::<Vec<InMemElement>>(StatusWarning::Warning.into(),&mut task,vec![])?;
				} else {
					cmd.respond(StatusOk::Success(vec![]).into(),&mut task,vec![])?;
				}
				Ok(())
			}

			_ => {todo!()}
		}
	}

	async fn fetch_obj(&mut self,mut prelude:Vec<PDataValue>, override_ts:Option<&'static str>) -> Result<InMemDicomObject>
	{
		let ts = override_ts.map(String::from).unwrap_or(self.selected_ts.clone());
		let mut buffer = Cursor::<Vec<u8>>::default();
		let mut last = false;
		while let Some(dat) = prelude.pop(){
			last = dat.is_last;
			buffer.write_all(&dat.data)?;
		}

		buffer.set_position(0);
		if last {
			let ts = TransferSyntaxRegistry.get(ts.as_str()).ok_or(DicomTransferSyntaxNotFound(ts))?;
			InMemDicomObject::read_dataset_with_ts(buffer,ts).map_err(to_dicom_err)
		} else {
			debug!("Creating object from multiple pdu");
			let (obj,source) = super::io::read_pipe(
				self.source.take().expect("No source"),
				buffer,	ts
			).await?;
			self.source = Some(source);
			Ok(obj)
		}
	}
	async fn receive_command(&mut self) -> Result<Command> {
		let rec=self.source.as_mut().expect("No source").recv().await.expect("pipe broken");
		self.read_command(vec![rec]).await
	}
	async fn read_command(&mut self, data:Vec<PDataValue>) -> Result<Command>
	{
		let pc_id = data[0].presentation_context_id;
		let mut obj = self.fetch_obj(data, Some(IMPLICIT_VR_LITTLE_ENDIAN.uid())).await?;

		let cmd = Command {
			id: obj.take(tags::COMMAND_FIELD).expect("Could not get command field").uint16().unwrap(),
			status: obj.take(tags::STATUS).map(|e|e.to_int()).transpose().expect("Could not get command status"),
			pc_id, obj,	succeeded: 0, warn: 0, fail: 0,	to_do: 0,
		};
		let sop_class=cmd.sop_class().ok()
			.and_then(|e|e.to_str().ok())
			.and_then(|uid|dicom_dictionary_std::sop_class::StandardSopClassDictionary.by_uid(&uid))
			.map_or("None",|uid|uid.name);
		let msgid=cmd.msgid().ok()
			.and_then(|e|e.to_str().ok())
			.map_or(Cow::from("None"),|s|s);
		debug!("Got command:{:04X}H msgid:{msgid} SOP Class:\"{sop_class}\"",cmd.id	);
		Ok(cmd)
	}
	fn send_command(&mut self, attr:impl IntoIterator<Item = InMemElement>, attachment:SendAttachment) -> Result<()> {
		self.sink.send(make_command(self.pc_id,attr, attachment).map_err(to_dicom_err)?)?;
		debug!("command is out");
		Ok(())
	}
	pub fn set_id(&mut self, id: u16)
	{
		self.id.0.store(id, Ordering::Release);
		self.id.1.store(true, Ordering::Release);
	}

	async fn c_store(&mut self, file: File, attr: impl IntoIterator<Item = InMemElement>) -> Result<()> {

		let mut rq =create_request(C_STORE_RQ,Some(rand::random::<u16>()));
		let mut source = OpenFileOptions::new().read_until(tags::AFFECTED_SOP_INSTANCE_UID)
			.open_file(file.get_path()).map_err(to_dicom_err)?;
		for (s_tag,d_tag) in [
			(tags::SOP_INSTANCE_UID,tags::AFFECTED_SOP_INSTANCE_UID),
			(tags::SOP_CLASS_UID,tags::AFFECTED_SOP_CLASS_UID)
		]
		{
			source.take_element(s_tag).map(InMemElement::into_value)
				.map(|v|rq.put(DataElement::new(d_tag,VR::UI,v)))
				.map_err(to_dicom_err).context(format!("File {} lacking tag {s_tag}",file.get_path().display()))?;
		}
		for attr in attr {rq.put(attr.clone());}
		self.send_command(rq,SendAttachment::File(file))?;

		loop {
			// wait for confirmation
			match self.receive_response(C_STORE_RQ).await?.status
			{
				Ok(StatusOk::Success(_)) => return Ok(()),
				Ok(StatusOk::Warning(e)) => {
					warn!("Unexpected Warning reply from c-store {e:?} ");
					return Ok(())
				},
				Ok(StatusOk::Pending(_)) => {},
				Err(e) => return Err(e.into())
			}
		}
	}

	async fn receive_response(&mut self, source_rq:u16) -> Result<Reply>{
		let reply = self.receive_command().await?;

		let counts:Vec<Option<u16>> = [
			tags::NUMBER_OF_REMAINING_SUBOPERATIONS,
			tags::NUMBER_OF_COMPLETED_SUBOPERATIONS,
			tags::NUMBER_OF_FAILED_SUBOPERATIONS,
			tags::NUMBER_OF_WARNING_SUBOPERATIONS
		].iter().map(|t|reply.obj
			.element_opt(t.clone()).unwrap()
			.and_then(|e|e.to_int().ok())
		).collect();

		let offending = reply.obj.element_opt(tags::OFFENDING_ELEMENT).unwrap()
			.map(|e|e.to_multi_str().expect("unexpected non-primitive"))
			.map(|e|e.into_iter().cloned().collect::<Vec<_>>()).unwrap_or_default();
		let comment = reply.obj.element_opt(tags::ERROR_COMMENT).unwrap()
			.and_then(|e|e.to_str().ok()).unwrap_or(Cow::Borrowed("")).to_string();

		let status = if reply.id != (source_rq|0x8000) {
			warn!("ignoring invalid reply {:04X}H", reply.id);
			Err(StatusFailure::Failure)
		} else if let Some(status) = reply.status {
			match match_status(status)
			{
				Ok(StatusOk::Success(_)) => Ok(StatusOk::Success(reply.obj).into()),
				Ok(StatusOk::Warning(w)) => Ok(StatusOk::Warning(w).into()),
				Ok(StatusOk::Pending(_)) => Ok(StatusOk::Pending(reply.obj).into()),
				Err(e) => Err(e.into())
			}
		} else {
			error!("reply is missing status field, assuming failure");
			Err(StatusFailure::Failure)
		};
		Ok(Reply{status,succeeded:counts[0],failed:counts[1],warning:counts[2]})
	}
}


impl Message

{
	pub fn new(
		data: Vec<PDataValue>,
		selected_ts:String,
		sink:tokio::sync::mpsc::UnboundedSender<SendPayload>
	) -> Message
	{
		assert!(!data.is_empty());
		assert_eq!(data[0].value_type,PDataValueType::Command);
		let (to_task, source) = tokio::sync::mpsc::unbounded_channel();
		let id:Arc<(AtomicU16,AtomicBool)>=Default::default();

		let task=MessageTask::run(
			data,
			id.clone(),
			selected_ts,
			source,
			sink,
		);
		Message{id, handle: tokio::spawn(task), to_task}
	}
	pub fn id(&self) -> Option<u16>
	{
		if self.id.1.load(Ordering::Relaxed) {Some(self.id.0.load(Ordering::Relaxed))} else {None}
	}
}

fn create_request(command:u16,msgid: Option<u16>) -> InMemDicomObject
{
	let msgid=msgid.unwrap_or(rand::random::<u16>());
	InMemDicomObject::command_from_element_iter([
		DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [command])),
		DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [msgid])),
		DataElement::new(tags::PRIORITY, VR::US, dicom_value!(U16, [0x0000])),
	])
}

fn make_command(presentation_context_id:u8, attr:impl IntoIterator<Item = InMemElement>, attachment:SendAttachment)
				-> std::result::Result<SendPayload,dicom::object::WriteError>
{
	let mut data = Vec::new();
	let mut obj= InMemDicomObject::command_from_element_iter(attr);

	if let SendAttachment::None = &attachment{
		obj.put(DataElement::new(tags::COMMAND_DATA_SET_TYPE,VR::US,dicom_value!(U16,0x0101)));
	}else{
		obj.put(DataElement::new(tags::COMMAND_DATA_SET_TYPE,VR::US,dicom_value!(U16,0x0000)));
	}

	obj.write_dataset_with_ts(&mut data, &IMPLICIT_VR_LITTLE_ENDIAN.erased())?;
	let command = PDataValue { presentation_context_id, value_type: PDataValueType::Command, is_last: true, data };
	Ok(SendPayload{command,attachment})
}
