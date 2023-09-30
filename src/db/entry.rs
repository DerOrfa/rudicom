use surrealdb::sql::Thing;
use crate::db::{json_to_thing, query_for_entry};
use crate::{JsonMap, JsonVal};
use anyhow::{anyhow, Result};
use self::Entry::{Instance, Series, Study};

pub(crate) enum Entry
{
	Instance(JsonMap),
	Series(JsonMap),
	Study(JsonMap)
}

impl Entry
{
	pub async fn query(id:Thing) -> Result<Entry>
	{
		if let JsonVal::Object(me) = query_for_entry(id).await?{
			Entry::try_from(me)
		}
		else {Err(anyhow!("Invalid query result for constructing an entry"))}
	}

	fn data(&self) -> &JsonMap
	{
		match self {
			Instance(v)| Series(v) | Study(v) => v
		}
	}

	pub fn get_id(&self) -> &str
	{
		self.data()
			.get("id").expect("Entry must have an id")
			.get("id").and_then(|id|id.get("String"))
			.and_then(|s|s.as_str()).expect("invalid id")
	}

	pub fn get_name(&self) -> String
	{
		match self {
			Instance(data) => {
				let number=data.get("InstanceNumber").and_then(|v|v.as_str()).unwrap_or("--");
				format!("Instance {number}")
			},
			Series(data) => {
				let number=data.get("SeriesNumber").and_then(|v|v.as_str()).unwrap_or("--");
				let desc= data.get("SeriesDescription").and_then(|v|v.as_str()).unwrap_or("--");
				format!("S{number}_{desc}")
			},
			Study(data) => {
				let id=data.get("PatientID").and_then(|v|v.as_str()).unwrap_or("--");
				let date=data.get("StudyDate").and_then(|v|v.as_str()).unwrap_or("--");
				let time=data.get("StudyTime").and_then(|v|v.as_str()).unwrap_or("--");
				format!("{id}/{date}_{time}")
			}
		}
	}

	pub fn remove(&mut self,key:&str)
	{
		match self {
			Instance(data) => data,
			Series(data) => data,
			Study(data) => data
		}.remove(key);
	}
}

impl TryFrom<JsonMap> for Entry
{
	type Error = anyhow::Error;

	fn try_from(json_entry: JsonMap) -> std::result::Result<Self, Self::Error> {
		let id=json_entry.get("id").ok_or(anyhow!(r#"no "id" in entry"#))?;
		let table= json_to_thing(id.clone())?.tb;

		Ok(match table.as_str() {
			"instances" => {Instance(json_entry)},
			"series" => {Series(json_entry)},
			"studies" => {Study(json_entry)}
			_ => {panic!("invalid table name")}
		})
	}
}
