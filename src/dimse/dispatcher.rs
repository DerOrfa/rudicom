use std::borrow::Cow;
use std::default::Default;
use std::io::{BufWriter, Cursor, Write};
use dicom::encoding::TransferSyntaxIndex;
use dicom::transfer_syntax::{TransferSyntaxRegistry};
use dicom_ul::association::server::{ServerAssociationOptions, ServerAssociation};
use dicom_ul::{AeAddr, FullAeAddr, Pdu};
use dicom_ul::pdu::{PDataValue, PDataValueType};
use std::net::SocketAddr;
use std::sync::{Arc};
use dicom::object::{open_file, InMemDicomObject};
use dicom::transfer_syntax::entries::{EXPLICIT_VR_LITTLE_ENDIAN, IMPLICIT_VR_LITTLE_ENDIAN};
use dicom_dictionary_std::tags;
use tokio::sync::{watch::Sender,Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::{spawn_blocking};
use tokio_util::io::SyncIoBridge;
use tracing::{debug, error, info, warn};
use crate::dimse::message::{to_dicom_err, Message};
use crate::dimse::payload::{SendAttachment, SendPayload};
use crate::tools;
use crate::tools::Result;
use crate::tools::error::DicomError;

enum OpenState{
	Open(Sender<()>),
	Closing,
	Closed
}
pub(crate) struct Dispatcher
{
	inner:Arc<Mutex<ServerAssociation<tokio::net::TcpStream>>>,
	title:FullAeAddr<SocketAddr>,
	client:FullAeAddr<SocketAddr>,
	state: OpenState,
	current:Option<Message>,
}

impl Dispatcher
{
	pub async fn lookup_ts(&self, presentation_context_id: u8) -> Result<String> {
		self.inner.lock().await.presentation_contexts().iter()
			.find(|pc| pc.id == presentation_context_id)
			.ok_or(DicomError::DicomInvalidPC(presentation_context_id))
			.map(|pc|
				pc.transfer_syntax.trim_end_matches('\0').to_string()
			).map_err(|e|e.into())
	}

	pub async fn new(ae_title:impl AsRef<str>, scu_stream: tokio::net::TcpStream, watcher:Sender<()>)  -> Result<Self>
	{

		let mut options = ServerAssociationOptions::new()
			.accept_any()
			.ae_title(ae_title.as_ref())
			.promiscuous(true);

		for ts in TransferSyntaxRegistry.iter() {
			if !ts.is_unsupported() {
				options = options.with_transfer_syntax(ts.uid());
			}
		}
		let client = AeAddr::from(scu_stream.peer_addr()?);
		let title = FullAeAddr::new(ae_title.as_ref(),scu_stream.local_addr()?);
		let inner = options.establish_async(scu_stream).await.map_err(DicomError::from)?;
		let client= client.with_ae_title(inner.client_ae_title());

		info!("New association from {client}");
		for pc in inner.presentation_contexts()
		{
			let ts = pc.transfer_syntax.trim_end_matches('\0');
			let ts = TransferSyntaxRegistry.get(ts).map_or(ts,|t|t.name());
			debug!("Presentation context {} => transfer syntax:\"{}\"", pc.id,ts);
		}
		Ok(Self{
			inner:Arc::new(inner.into()),	client,	title,
			state: OpenState::Open(watcher),
			current: None
		})
	}
	async fn next(&mut self, sink: &UnboundedSender<SendPayload>) -> Result<()>{
		let pdu = match &mut self.state {
			OpenState::Open(watcher) => { // normal state receive data while watching for command to shut down
				let mut inner = self.inner.lock().await;
				tokio::select! {
					r = inner.receive() => Some(r.map_err(DicomError::from)?),
					_ = watcher.closed() => None
				}
			},
			OpenState::Closing => // closing, but still accepting input
				self.inner.lock().await.receive().await.map_err(DicomError::from).map(|p|Some(p))?,
			OpenState::Closed => return Ok(()), // actually should never happen, as outer tool terminates on "Closed"
		};
		if let Some(pdu) = pdu {
			self.next_command(sink,pdu).await?;
		} else { // watcher told us to shut down - send release request
			debug!("Asking {} to release connection", self.client);
			self.inner.lock().await.send(&Pdu::ReleaseRQ).await.map_err(DicomError::from)?;
			self.state = OpenState::Closing;
		}
		Ok(())
	}
	async fn next_command(&mut self, sink:&UnboundedSender<SendPayload>, pdu:Pdu) -> Result<()>
	{
		match pdu{
			Pdu::PData { data } => self.dispatch(sink,data).await?,
			Pdu::ReleaseRQ => {
				debug!("{} asked to release the connection, closing ..", self.client);
				self.inner.lock().await.send(&Pdu::ReleaseRP).await.map_err(DicomError::from)?;
				self.state = OpenState::Closed;
			}
			Pdu::ReleaseRP => {
				self.state=OpenState::Closed;
				debug!("{} confirmed release. Bye...", self.client);
			},
			Pdu::AbortRQ { source } => {
				warn!("Aborted connection from: {:?}", source);
				self.state=OpenState::Closed;
			}
			Pdu::AssociationRQ(_)|Pdu::AssociationAC(_)|Pdu::AssociationRJ(_)|Pdu::Unknown { .. } =>
				warn!("Ignoring unexpected {} from {}",pdu.short_description(), self.client),
		};
		Ok(())
	}
	async fn fetch_command_obj(&mut self,prelude:&Vec<PDataValue>) -> Result<InMemDicomObject>
	{
		assert!(!prelude.is_empty());
		let mut buffer = Cursor::<Vec<u8>>::default();
		let mut last = false;
		for dat in prelude{
			last = dat.is_last;
			buffer.write_all(&dat.data)?;
		}

		buffer.set_position(0);
		if last {
			InMemDicomObject::read_dataset_with_ts(buffer,&IMPLICIT_VR_LITTLE_ENDIAN.erased()).map_err(to_dicom_err)
		} else {
			todo!()
		}
	}

	async fn peek_command(&mut self, data:&Vec<PDataValue>) -> Result<crate::dimse::message::Command>
	{
		let pc_id = data[0].presentation_context_id;
		let mut obj = self.fetch_command_obj(data).await?;

		let cmd = crate::dimse::message::Command {
			id: obj.take(tags::COMMAND_FIELD).expect("Could not get command field").uint16().unwrap(),
			status: obj.take(tags::STATUS).map(|e|e.to_int()).transpose().expect("Could not get command status"),
			pc_id, obj,	succeeded: 0, warn: 0, fail: 0,	to_do: 0,
		};
		let msgid=cmd.msgid().ok()
			.and_then(|e|e.to_str().ok())
			.map_or(Cow::from("None"),|s|s);
		debug!("Peaked command:{:04X}H msgid:{msgid}",cmd.id);
		Ok(cmd)
	}

	async fn dispatch(&mut self,sink:&UnboundedSender<SendPayload>,data:Vec<PDataValue>) -> Result<()>
	{
		if let Some(first) = data.first()
		{
			match first.value_type {
				PDataValueType::Command => {
					let peek = self.peek_command(&data).await?;
					if (peek.id & 0x8000u16) == 0{ // not a reply
						let selected_ts = self.lookup_ts(first.presentation_context_id).await?;
						debug!("Dispatching new message");
						let message = Message::new(data, selected_ts, sink.clone());
						self.current = Some(message);
						return Ok(());
					}
				},
				_ => {}
			}
			if let Some(msg) = self.current.as_mut()
			{
				// if let Some(id) = msg.id(){debug!("Dispatching data for message {id}");}
				// else {debug!("Dispatching new data");}
				for d in data{
					msg.to_task.send(d)?;
				}
			} else {
				debug!("Ignoring unexpected data or replies");
			}

		}
		else
		{
			error!("Unexpectedly got empty pdu");
		}
		Ok(())
	}
	async fn send_payload(&mut self, payload: SendPayload)-> Result<()>
	{
		debug!("Sending payload to {}",self.client);
		let pc = payload.command.presentation_context_id;
		self.inner.lock().await.send(&Pdu::PData {data:vec![payload.command]}).await.unwrap();
		let obj = match payload.attachment {
			SendAttachment::File(f) => {
				let ts = self.lookup_ts(pc).await?;
				open_file(f.get_path()).map_err(DicomError::from)?.into_inner()
			}
			SendAttachment::Obj(o) => o,
			SendAttachment::None => return Ok(())
		};
		let inner = self.inner.clone();
		spawn_blocking(move || {
			let mut inner = inner.blocking_lock_owned();
			let sink = SyncIoBridge::new(inner.send_pdata(pc));
			obj.write_dataset_with_ts(BufWriter::new(sink),&EXPLICIT_VR_LITTLE_ENDIAN.erased())
		}).await.expect("unexpected join error").map_err(|e|tools::Error::DicomError(e.into()))

	}
	pub(crate) async fn run(mut self) -> Result<()>
	{
		let (sink, mut from_tasks) = tokio::sync::mpsc::unbounded_channel();
		loop {
			tokio::select! {
				n = self.next(&sink) => {},
				Some(payload) = from_tasks.recv() => {
					self.send_payload(payload).await?;
				}
			};
			if let OpenState::Closed = self.state {break}
		}
		info!("association {} => {} is closed", self.client, self.title);
		Ok(())
	}
}
