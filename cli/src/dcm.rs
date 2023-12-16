use std::str::FromStr;
use dicom::core::{DataDictionary, Tag};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use crate::config;
use crate::db::IntoDbValue;
use std::fmt::Write;
use std::ops::Deref;
use surrealdb::sql;
use dicom::core::header::HasLength;
use strfmt::{FmtError, strfmt_map};

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

pub fn extract(obj: &DefaultDicomObject, requested:Vec<(String, Tag)>) -> Vec<(String, sql::Value)>
{
	requested.into_iter()
		.map(|(name,tag)|(name,obj.element_opt(tag).unwrap()))
		.map(|(k,v)|(k,v.cloned().into_db_value()))
		.collect()
}

pub fn gen_filepath(obj:&DefaultDicomObject) -> strfmt::Result<String>
{
	let pattern:String = config::get("filename_pattern").expect(r#""filename_pattern"  missing or invalid in config"#);
	strfmt_map(pattern.as_str(),|f| format_filepath(f, &obj))
}

fn format_filepath(mut f:strfmt::Formatter, obj:&DefaultDicomObject) -> strfmt::Result<()>
{
	let key = find_tag(f.key).ok_or(FmtError::KeyError(format!(r#"Tag "{}" is not known"#,f.key)))?;
	let val = obj.element_opt(key).unwrap();
	if val.is_none(){
		return f.write_str("<<none>>").map_err(|e|FmtError::Invalid(e.to_string()))
	}
	let val = val.unwrap();
	if val.is_empty(){
		return f.write_str("<<empty>>").map_err(|e|FmtError::Invalid(e.to_string()))
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
