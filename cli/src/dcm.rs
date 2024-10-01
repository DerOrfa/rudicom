use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use crate::config;
use crate::db::IntoDbValue;
use std::fmt::Write;
use std::ops::Deref;
use dicom::core::header::HasLength;
use itertools::Itertools;
use strfmt::{FmtError, strfmt_map};
use crate::tools::Context;

pub fn find_tag(name:&str) -> Option<Tag>
{
	StandardDataDictionary::default()
		.by_name(name)
		.map(|t|t.tag.inner())
		.or_else(||Tag::from_str(name).ok())
}

pub fn get_attr_list(config_key:&str, must_have:Vec<&str>) -> Vec<(String,Tag)>
{
	config::get::<Vec<String>>(config_key)//get tag list from config
		.expect(format!(r#"failed getting {config_key} from the config"#).as_str())
		.iter().map(|s| //without formatting
			s.split_once(':').unwrap_or((s,"")).0.to_string()
		)
		.chain(must_have.into_iter().map(|s|s.to_string()))//add must_have
		.sorted().dedup()
		.map(|name|{ //if tag doesn't exist that's a critical error caused by the config of program logic
			let tag = find_tag(name.as_str())
				.expect(format!("Tag {name} not found in dictionary").as_str());
			(name, tag)
		})
		.collect()
}

pub fn extract<'a>(obj: &DefaultDicomObject, requested:&'a Vec<(String, Tag)>) -> Vec<(&'a str, surrealdb::Value)>
{
	requested.iter()
		.map(|(k,tag)|(k.as_str(),obj.element_opt(tag.clone()).unwrap()))
		.map(|(k,v)|(k,surrealdb::Value::from_inner(v.cloned().into_db_value())))
		.collect()
}

pub fn gen_filepath(obj:&DefaultDicomObject) -> crate::tools::Result<String>
{
	let pattern:String = config::get("filename_pattern").expect(r#""filename_pattern"  missing or invalid in config"#);
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
