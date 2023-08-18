use std::collections::HashMap;
use anyhow::anyhow;
use surrealdb::sql::Thing;
use crate::JsonVal;

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
			json_to_thing(&value)
				.map(|id|HtmlItem::Id(id))
				.map_err(|_|anyhow!("invalid value (non-id object)"))
		} else {
			match value.to_owned() {
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

fn json_to_thing(v:&JsonVal) -> anyhow::Result<Thing>{
	let tb = v.get("tb").ok_or(anyhow!("expected tb in {v}"))?
		.as_str().ok_or(anyhow!("tb in {v} should be a string"))?;
	let id = v
		.get("id").and_then(|id|id.get("String"))
		.ok_or(anyhow!("expected id:String in {v}"))?
		.as_str().ok_or(anyhow!("id:String in {v} should be a string"))?;
	Ok(Thing::from((tb,id)))
}

pub(crate) fn make_item_map(map:JsonVal) -> anyhow::Result<HashMap<String,HtmlItem>>
{
	let map = map.as_object().ok_or(anyhow!("json value must be an object"))?;
	map.into_iter().map(|(k,v)|{
		let item=HtmlItem::try_from(v.to_owned())?;
		Ok((k.to_owned(),item))
	}).collect()
}
