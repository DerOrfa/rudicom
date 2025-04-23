use chrono::{DateTime, Utc};
use dicom::core::{DataElement, VR};
use dicom::dictionary_std::{tags, uids};
use dicom::object::{FileDicomObject, FileMetaTableBuilder, InMemDicomObject};
use std::time::SystemTime;

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
		(tags::STUDY_DATE, VR::DA, "20250101"),
		(tags::STUDY_TIME, VR::TM, "000000.000000"),
		(tags::STUDY_ID, VR::SH, "John_Doe"),
		(tags::SOP_CLASS_UID, VR::UI, uids::MR_IMAGE_STORAGE),
		(tags::PATIENT_NAME, VR::PN,"Doe^John",),
		(tags::PATIENT_ID, VR::LO,"John Doe",),
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
