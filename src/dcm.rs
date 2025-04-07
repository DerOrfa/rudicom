use crate::{config, db};
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
use strfmt::{strfmt_map, FmtError};

#[derive(Debug,Clone,Hash,PartialEq,Eq)]
pub struct AttributeSelector(pub dicom::core::ops::AttributeSelector);

impl From<Tag> for AttributeSelector{
	fn from(value: Tag) -> Self {AttributeSelector(value.into())}
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

pub fn extract<'a>(obj: &DefaultDicomObject, requested:&'a HashMap<String,Vec<AttributeSelector>>) -> Vec<(&'a str, surrealdb::Value)>
{
	requested.iter().map(|(k,selectors)|(
		k.deref(),
		selectors.iter()
			.find_map(|s|obj.entry_at(s.0.clone()).ok())
			.map(|v|v.clone().into_db_value())
			.unwrap_or_default()
		)
	)
	.map(|(k,v)|(k,surrealdb::Value::from_inner(v)))
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
	let val=val.to_str().map_err(|e|FmtError::Invalid(e.to_string()))?;
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
