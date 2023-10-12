use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use crate::config;
use crate::db::IntoDbValue;
use runtime_format::{FormatArgs, FormatKey, FormatKeyError};
use core::fmt;
use std::borrow::Cow;
use dicom::core::header::HasLength;
use crate::DbVal;

struct DicomAdapter<'a>(&'a DefaultDicomObject);

pub fn find_tag(name:&str) -> Option<Tag>
{
	StandardDataDictionary::default()
		.by_name(name)
		.map(|t|t.tag.inner())
		.or_else(||Tag::from_str(name).ok())
}

pub fn get_attr_list(config_key:&str, must_have:Vec<&str>) -> Vec<(String,Tag)>
{
	let mut config = config::get::<Vec<String>>(config_key)
		.expect(format!(r#"failed getting {config_key} from the config"#).as_str());
	config.extend(must_have.into_iter().map(|s|s.to_string()));
	config.sort();
	config.dedup();
	config.into_iter()
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

pub fn gen_filepath(obj:&DefaultDicomObject) -> String
{
	let pattern:String = config::get("filename_pattern").expect(r#""filename_pattern"  missing or invalid in config"#);
	FormatArgs::new(pattern.as_str(),&DicomAdapter(obj)).to_string()
}

impl<'a> FormatKey for DicomAdapter<'a> {
	fn fmt(&self, key: &str, f: &mut fmt::Formatter<'_>) -> Result<(), FormatKeyError> {
		if let Some(key) = find_tag(key){
			let val= self.0.element_opt(key).unwrap()
				.map_or(Cow::from("<<none>>"),|e|
					if e.is_empty(){
						Cow::from("<<empty>>")
					} else {
						e.to_str().unwrap_or(Cow::from("<<invalid>>"))
					}
				);
			write!(f,"{}",val).map_err(FormatKeyError::Fmt)
		}
		else {
			Err(FormatKeyError::UnknownKey)
		}
	}
}
