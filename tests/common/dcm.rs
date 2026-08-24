use std::io::ErrorKind;
use chrono::{DateTime, Utc};
use dicom::core::{DataElement, PrimitiveValue, VR};
use dicom::dictionary_std::{tags, uids};
use dicom::object::{FileDicomObject, FileMetaTableBuilder, InMemDicomObject};
use rudicom::{db, tools};
use rudicom::tools::remove::remove;
use rudicom::db::{RegisterResult};
use std::time::SystemTime;
use dicom::core::header::Header;
use dicom::core::value::Value;
use dicom::object::mem::InMemElement;
use tokio::task::JoinSet;
use tracing::trace;
use rudicom::db::register_manager::RegisterManager;
use rudicom::storage::Image;
use rudicom::tools::Error;

pub struct UidSynthesizer{
	prefix: String,
	dev_type: u16,
	dev_sn: u16,
	timestamp: DateTime<Utc>,
}

impl UidSynthesizer{
	fn new(ansi_member:u16,country_code:u16,org_code:u16,dev_type:u16,dev_sn:u16)->Self{
		UidSynthesizer{
			prefix: format!("1.{ansi_member}.{country_code}.{org_code}"),
			dev_type, dev_sn, timestamp: SystemTime::now().into(),
		}
	}
	pub fn instance(&self, stdy_num:u16, ser_num:u16, image_num:u16)->String{
		let random = rand::random::<u16>();
		format!("{}.{}.{}.{stdy_num}.{ser_num}.{image_num}.{}{random:05}",
				self.prefix,
				self.dev_type,
				self.dev_sn,
				self.timestamp.format("%Y%m%d%H%M%S")
		)
	}
	pub fn series(&self, stdy_num:u16, ser_num:u16)->String{
		format!("{}.{}.{}.{stdy_num}.{ser_num}.{}",
				self.prefix,
				self.dev_type,
				self.dev_sn,
				self.timestamp.format("%Y%m%d%H%M%S")
		)
	}
	pub fn study(&self, stdy_num:u16)->String{
		format!("{}.{}.{}.{stdy_num}.{}",
				self.prefix,
				self.dev_type,
				self.dev_sn,
				self.timestamp.format("%Y%m%d%H%M%S")
		)
	}
}

impl Default for UidSynthesizer {
	fn default() -> Self {
		UidSynthesizer::new(3,12,12,1107,5)
	}
}

pub fn synthesize_dicom_obj(uid_synthesizer: &UidSynthesizer, stdy_num:u16, ser_num:u16, image_num:u16) -> FileDicomObject<InMemDicomObject> {
	let str_tags = [
		(tags::SERIES_DATE, VR::DA, "20250101"),
		(tags::SERIES_TIME, VR::TM, "000000.000000"),
		(tags::STUDY_DATE, VR::DA, "20250101"),
		(tags::STUDY_TIME, VR::TM, "000000.000000"),
		(tags::STUDY_ID, VR::SH, "John_Doe_Study"),
		(tags::SOP_CLASS_UID, VR::UI, uids::MR_IMAGE_STORAGE),
		(tags::PATIENT_NAME, VR::PN,"Doe^John"),
		(tags::PATIENT_ID, VR::LO,"John_Doe"),
		(tags::MODALITY, VR::CS,"MR"),
	].map(|(id,vr,val)|DataElement::new(id,vr,val)).into_iter();
	let string_tags = [
		(tags::SOP_INSTANCE_UID, VR::UI, uid_synthesizer.instance(stdy_num,ser_num,image_num)),
		(tags::SERIES_INSTANCE_UID, VR::UI, uid_synthesizer.series(stdy_num,ser_num)),
		(tags::STUDY_INSTANCE_UID,  VR::UI, uid_synthesizer.study(stdy_num)),
		(tags::SERIES_NUMBER, VR::IS, ser_num.to_string()),
		(tags::INSTANCE_NUMBER, VR::IS, image_num.to_string()),
	].map(|(id,vr,val)|DataElement::new(id,vr,val)).into_iter();
	InMemDicomObject::from_element_iter(str_tags.chain(string_tags))
		.with_meta(FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)).unwrap()

}

pub fn synthesize_series(uid_synthesizer: &UidSynthesizer, stdy_num:u16, ser_num:u16, instances:u16) -> Vec<FileDicomObject<InMemDicomObject>>
{
	(0..instances)
		.map(|i|synthesize_dicom_obj(uid_synthesizer,stdy_num,ser_num,i)).collect()
}

pub fn synthesize_study(uid_synthesizer: &UidSynthesizer, stdy_num:u16, series:u16, instances:u16) -> Vec<Vec<FileDicomObject<InMemDicomObject>>>
{
	(0..series)
		.map(|i|synthesize_series(uid_synthesizer,stdy_num,i,instances)).collect()
}

pub async fn bulk_insert(instances:impl Iterator<Item=&FileDicomObject<InMemDicomObject>>) 
	-> tools::Result<Vec<RegisterResult>>
{
	let mut tasks = JoinSet::new();
	let mut session = RegisterManager::new();

	for obj in instances.cloned(){
		// register all instances, will block once the buffer runs full
		let store = session.register(Image::from_obj(obj)).await?;
		// put all Receivers into a JoinSet and let them wait
		tasks.spawn(store);
	}
	// collect all results, there cannot be any register pending => no flush necessary
	tasks.join_all().await.into_iter()
		.map(|r|r.unwrap_or_else(|e|Err(Error::IoError(std::io::Error::new(ErrorKind::BrokenPipe,e)))))
		.collect()
}

pub async fn cleanup() -> rudicom::tools::Result<()>
{
	let studies= db::list_entries("instances").await?;
	for study in studies{
		trace!("removing leftover {}", study.id());
		remove(study.id()).await?;
	}
	Ok(())
}

pub fn diff(obj1:InMemDicomObject, mut obj2:InMemDicomObject)-> Vec<(Option<InMemElement>, Option<InMemElement>)>
{
	obj1.into_iter()
		.filter_map(|org|{
			let got = obj2.take(org.tag());
			match got {
				Some(g) => {
					let same = if let Value::Primitive(PrimitiveValue::Str(_)) = org.value(){
						org.to_str().unwrap() == g.to_str().unwrap()
					} else {
						org.value() == g.value()
					};
					if same {None} else {Some((Some(org),Some(g)))}
				}
				None => Some((Some(org), None)),
			}
		})
		.collect()
}
