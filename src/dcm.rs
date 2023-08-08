use std::collections::BTreeMap;
use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary, mem::InMemElement};
use once_cell::sync::Lazy;

pub static INSTACE_TAGS:Lazy<Vec<(&str, Tag)>> = Lazy::new(
	||get_attr_list(vec!["InstanceCreationDate", "InstanceCreationTime", "InstanceNumber"])
);
pub static SERIES_TAGS:Lazy<Vec<(&str, Tag)>> = Lazy::new(
	||get_attr_list(vec!["ProtocolName", "SequenceName", "SeriesDate", "SeriesTime", "SeriesDescription", "SeriesNumber"])
);
pub static STUDY_TAGS:Lazy<Vec<(&str, Tag)>> = Lazy::new(
	||get_attr_list(vec!["StudyTime", "StudyDate", "StudyDescription", "OperatorsName", "ManufacturerModelName"])
);

fn get_attr_list(names:Vec<&str>) -> Vec<(&str,Tag)>{
	let mut request = Vec::new();
	for name in names{
		let tag = StandardDataDictionary::default()
			.by_name(name)
			.map(|t|t.tag.inner())
			.or_else(||Tag::from_str(name).ok());
		match tag {
			None => {eprintln!("No tag found for {name}");}
			Some(t) => {request.push((name, t));}
		}
	}
	request
}

pub fn extract<'a,'b>(obj:&'a DefaultDicomObject, requested:Vec<(&'b str,Tag)>) -> BTreeMap<&'b str,Option<&'a InMemElement>>
{
	requested.into_iter()
		.map(|(name,tag)|(name,obj.element_opt(tag).unwrap()))
		.collect()
}
