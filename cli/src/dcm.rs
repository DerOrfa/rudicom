use crate::config;
use crate::db::IntoDbValue;
use crate::tools::Context;
use dicom::core::header::HasLength;
use dicom::core::ops::AttributeSelector;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use itertools::Itertools;
use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Deref;
use std::str::FromStr;
use strfmt::{strfmt_map, FmtError};

pub fn find_tag(name:&str) -> Option<Tag>
{
	StandardDataDictionary::default()
		.by_name(name)
		.map(|t|t.tag.inner())
		.or_else(||Tag::from_str(name).ok())
}

pub fn get_attr_list(config_key:&str, must_have:Vec<(&str,Vec<&str>)>) -> HashMap<String,Vec<AttributeSelector>>
{
	let dict = StandardDataDictionary::default();
	let mut attrs = config::get::<HashMap<String,Vec<String>>>(config_key)//get tag list from config
		.expect(format!(r#"failed getting "{config_key}" from the config"#).as_str());
	// add "must have" by taking out each (or default), chain, and put back
	for (label, need) in must_have 
	{
		let need:Vec<_> = need.into_iter()
			.map(str::to_string)
			.chain(attrs.remove(label).unwrap_or(Default::default()))
			.unique().collect(); 
		attrs.insert(label.to_string(),need);
	}
	let mut ret= HashMap::<String,Vec<AttributeSelector>>::default();
	for (label, attr) in attrs
	{
		let attr:Vec<_> = attr.into_iter().map(|s|
		   dict.parse_selector(&s).expect(format!("Tag {label} not found in dictionary").as_str())
		).collect();
		ret.insert(label,attr);
	}
	ret
}

pub fn extract<'a>(obj: &DefaultDicomObject, requested:&'a HashMap<String,Vec<AttributeSelector>>) -> Vec<(&'a str, surrealdb::Value)>
{
	requested.iter().map(|(k,selectors)|(
		k.deref(),
		selectors.iter()
			.find_map(|s|obj.entry_at(s.clone()).ok())
			.map(|v|v.clone().into_db_value())
			.unwrap_or_default()
		)
	)
	.map(|(k,v)|(k,surrealdb::Value::from_inner(v)))
	.collect()
}

pub fn gen_filepath(obj:&DefaultDicomObject) -> crate::tools::Result<String>
{
	let pattern:String = config::get("paths.filename_pattern").expect(r#""filename_pattern"  missing or invalid in config"#);
	strfmt_map(pattern.as_str(),|f| format_filepath(f, &obj))
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
