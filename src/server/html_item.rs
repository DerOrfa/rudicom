use std::collections::HashMap;
use anyhow::{Context,anyhow};
use surrealdb::sql::Thing;
use crate::{db, JsonVal};

pub(crate) enum HtmlItem {
	Bool(bool),
	Number(serde_json::Number),
	String(String),
	Id(Thing),
	Array(Vec<HtmlItem>)
}

impl TryFrom<JsonVal> for HtmlItem
{
	type Error = anyhow::Error;
	fn try_from(value: JsonVal) -> Result<Self, Self::Error> {
		if value.is_object() {
			db::json_to_thing(value)
				.map(|id|HtmlItem::Id(id))
				.context("failed parsing json as id object")
		} else {
			match value {
				JsonVal::Bool(b) => Ok(HtmlItem::Bool(b)),
				JsonVal::Number(n) => Ok(HtmlItem::Number(n)),
				JsonVal::String(s) => Ok(HtmlItem::String(s)),
				JsonVal::Array(mut a) => {
					let vec:Result<Vec<_>,Self::Error>=a.drain(0..).map(|v|HtmlItem::try_from(v)).collect();
					Ok(HtmlItem::Array(vec?))
				}
				_ => Err(anyhow!("invalid value {value:#?}")),
			}
		}
	}
}

impl ToString for HtmlItem
{
	fn to_string(&self) -> String {
		match self {
			HtmlItem::Bool(b) => if *b {"True".into()} else {"False".into()},
			HtmlItem::Number(n) => n.to_string(),
			HtmlItem::String(s) => s.to_owned(),
			HtmlItem::Id(id) => format!("{}:{}",id.tb,id.id),
			HtmlItem::Array(a) => {
				let list:Vec<_> = a.iter().map(|i|i.to_string()).collect();
				list.join("\n")
			}
		}
	}
}

pub(crate) fn make_item_map(val:JsonVal) -> anyhow::Result<HashMap<String,HtmlItem>>
{
	if let JsonVal::Object(map) = val {
		map.into_iter().map(|(k,v)|{
			let item=HtmlItem::try_from(v)?;
			Ok((k,item))
		}).collect()
	} else {
		Err(anyhow!("json value {val} must be an object"))
	}
}
