use std::collections::BTreeMap;
use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary, mem::InMemElement};
use once_cell::sync::Lazy;
use crate::config;

pub static INSTACE_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("instace_tags"));
pub static SERIES_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("series_tags"));
pub static STUDY_TAGS:Lazy<Vec<(String, Tag)>> = Lazy::new(||get_attr_list("study_tags"));

fn get_attr_list(config_key:&str) -> Vec<(String,Tag)>
{
	config::get::<Vec<String>>(config_key).unwrap().into_iter()
		.filter_map(|name|{
			let tag = StandardDataDictionary::default()
				.by_name(name.as_str())
				.map(|t|t.tag.inner())
				.or_else(||Tag::from_str(name.as_str()).ok());
			if tag.is_none(){eprintln!("No tag found for {name}");}
			tag.map(|t|(name, t))
		})
		.collect()
}

pub fn extract(obj: &DefaultDicomObject, requested:Vec<(String, Tag)>) -> BTreeMap<String, Option<&InMemElement>>
{
	requested.into_iter()
		.map(|(name,tag)|(name,obj.element_opt(tag).unwrap()))
		.collect()
}
