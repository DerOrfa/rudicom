use crate::{config, db, tools};
use crate::db::IntoDbValue;
use crate::tools::Context;
use dicom::core::header::HasLength;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::LazyLock;
use strfmt::{strfmt_map, FmtError};
use surrealdb::types as db_types;

#[derive(Debug,Clone,Hash,PartialEq,Eq)]
pub enum AttributeSelector{
	Core(dicom::core::ops::AttributeSelector),
	CSA {
		base: dicom::core::ops::AttributeSelector,
		element: String,
	}
}

pub static INSTANCE_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> =
	LazyLock::new(|| get_attr_list(db::Table::Instances, vec![("Number", vec![Tag::from((0x0020,0x0013))])]));//InstanceNumber
pub static SERIES_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> =
	LazyLock::new(|| get_attr_list(db::Table::Series, vec![
		("Description",vec![Tag::from((0x0008,0x103E))]), //SeriesDescription
		("Number",vec![Tag::from((0x0020,0x0011))]) // SeriesNumber
	])
	);
pub static STUDY_TAGS: LazyLock<HashMap<String, Vec<AttributeSelector>>> =
	LazyLock::new(|| get_attr_list(db::Table::Studies, vec![
		("Name",vec![Tag::from((0x0010,0x0010))]),//PatientName
		("Time", vec![Tag::from((0x0008,0x0030))]), // StudyTime 
		("Date", vec![Tag::from((0x0008,0x0020))]) // StudyDate
	])
	);

impl From<Tag> for AttributeSelector{
	fn from(value: Tag) -> Self {AttributeSelector::Core(value.into())}
}

pub fn find_tag(name:&str) -> Option<Tag>
{
	StandardDataDictionary::default()
		.by_name(name)
		.map(|t|t.tag.inner())
		.or_else(||Tag::from_str(name).ok())
}

pub fn get_attr_list(table:db::Table, must_have:Vec<(&str,Vec<Tag>)>) -> HashMap<String,Vec<AttributeSelector>>
{
	let mut attrs = match table {
		db::Table::Studies => config::get().study_tags.clone(),
		db::Table::Series => config::get().series_tags.clone(),
		db::Table::Instances => config::get().instance_tags.clone()
	};
	
	// add "must have" by taking out each (or default), chain, and put back
	for (label, need) in must_have 
	{
		let need:Vec<_> = need.into_iter().map_into::<AttributeSelector>()
			.chain(attrs.remove(label).unwrap_or(Default::default()))
			.unique().collect(); 
		attrs.insert(label.to_string(),need);
	}
	attrs
}

pub fn extract<'a>(obj: &DefaultDicomObject, requested:&'a HashMap<String,Vec<AttributeSelector>>) -> Vec<(&'a str, db_types::Value)>
{
	requested.iter().map(|(k,selectors)|(
		k.deref(),
		selectors.iter()
			.find_map(|s| {
				match s {
					AttributeSelector::Core(selector) => 
						obj.entry_at(selector.clone()).map(|e|e.clone().into_db_value()).ok(),
					AttributeSelector::CSA{ base, element } => 
						obj.entry_at(base.clone()).ok()
							.and_then(|b|tools::csa::extract_csa(b, element).unwrap())
							.map(|e|e.into_db_value()),					
				}
			})
			.unwrap_or_default()
		)
	)
	.filter(|(_,v)|!v.is_none())
	.collect()
}

pub fn gen_filepath(obj:&DefaultDicomObject) -> crate::tools::Result<String>
{
	let pattern = config::get().paths.filename_pattern.as_str();
	strfmt_map(pattern,|f| format_filepath(f, &obj))
		.context(format!("generating filename using pattern '{pattern}'"))
}

fn format_filepath(mut f:strfmt::Formatter, obj:&DefaultDicomObject) -> strfmt::Result<()>
{
	let key = find_tag(f.key).ok_or(FmtError::KeyError(format!(r#"Tag "{}" is not known"#,f.key)))?;
	let val = obj.element_opt(key).unwrap();
	if val.is_none(){
		return f.write_str("__none__").map_err(|e|FmtError::Invalid(e.to_string()))
	}
	let val = val.unwrap();
	if val.is_empty(){
		return f.write_str("__empty__").map_err(|e|FmtError::Invalid(e.to_string()))
	}
	let val=val.to_str().map_err(|e|FmtError::Invalid(e.to_string()))?
		.trim_matches(' ').replace(['/',' '],"_");
	let mut val=val.deref();
	if let Some(width)=f.width()
	{
		match f.align()
		{
			strfmt::Alignment::Left => {//"<" -> shrink from the right side
				if val.len()>width{
					val= &val[..width];
				}
			}
			strfmt::Alignment::Right => {//">" -> shrink from the left side
				if val.len()>width{
					val= &val[val.len() - width..];
				}
			}
			_ => {}
		}
	}
	f.write_str(val).map_err(|e|FmtError::Invalid(e.to_string()))
}
