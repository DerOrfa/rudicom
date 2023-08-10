use std::path::PathBuf;
use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use once_cell::sync::Lazy;
use crate::config;
use crate::db::{DbVal, IntoDbValue};
use runtime_format::{FormatArgs, FormatKey, FormatKeyError};
use core::fmt;
use std::borrow::Cow;

pub static INSTACE_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("instace_tags"));
pub static SERIES_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("series_tags"));
pub static STUDY_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("study_tags"));


struct DicomAdapter<'a>(&'a DefaultDicomObject);

pub fn find_tag(name:&str) -> Option<Tag>
{
	StandardDataDictionary::default()
		.by_name(name)
		.map(|t|t.tag.inner())
		.or_else(||Tag::from_str(name).ok())
}

fn get_attr_list(config_key:&str) -> Vec<(String,Tag)>
{
	config::get::<Vec<String>>(config_key).unwrap().into_iter()
		.filter_map(|name|{
			let tag = find_tag(name.as_str());
			if tag.is_none(){eprintln!("No tag found for {name}");}
			tag.map(|t|(name, t))
		})
		.collect()
}

pub fn extract(obj: &DefaultDicomObject, requested:Vec<(String, Tag)>) -> Vec<(String, DbVal)>
{
	requested.into_iter()
		.map(|(name,tag)|(name,obj.element_opt(tag).unwrap()))
		.map(|(k,v)|(k,v.cloned().into_db_value()))
		.collect()
}

pub fn gen_filepath(obj:&DefaultDicomObject) -> PathBuf
{
	let root:PathBuf = config::get("storage_path").expect(r#""storage_path" missing or invalid in config"#);
	let pattern:String = config::get("filename_pattern").expect(r#""filename_pattern"  missing or invalid in config"#);
	root.join(
		FormatArgs::new(pattern.as_str(),&DicomAdapter(obj)).to_string()
	)
}

impl<'a> FormatKey for DicomAdapter<'a> {
	fn fmt(&self, key: &str, f: &mut fmt::Formatter<'_>) -> Result<(), FormatKeyError> {
		if let Some(key) = find_tag(key){
			let val= self.0.element_opt(key).unwrap()
				.map_or(Cow::from("<<none>>"),|e|e.to_str()
					.unwrap_or(Cow::from("<<invalid>>")));
			write!(f,"{}",val).map_err(FormatKeyError::Fmt)
		} else {Err(FormatKeyError::UnknownKey)}
	}
}
