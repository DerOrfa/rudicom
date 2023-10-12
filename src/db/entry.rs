use surrealdb::sql;
use crate::db;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde::{Serialize, Serializer};
use serde::ser::SerializeMap;
use surrealdb::sql::Thing;
use self::Entry::{Instance, Series, Study};

pub(crate) enum Entry
{
	Instance((sql::Thing,sql::Object)),
	Series((sql::Thing,sql::Object)),
	Study((sql::Thing,sql::Object))
}

impl Entry
{
	pub fn get(&self, key:&str) -> Option<&sql::Value>
	{
		self.data().1.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<String>
	{
		self.get(key).map(|v|v.to_raw_string())
	}
	fn data(&self) -> &(Thing,sql::Object)
	{
		match self {
			Instance(data)| Series(data) | Study(data) => data
		}
	}
	pub fn id(&self) -> &Thing	{&self.data().0}

	pub fn get_name(&self) -> String
	{
		match self {
			Instance(_) => {
				let number=self.get_string("InstanceNumber").unwrap_or("--".to_string());
				format!("Instance {number}")
			},
			Series(_) => {
				let number=self.get_string("SeriesNumber").unwrap_or("--".to_string());
				let desc= self.get_string("SeriesDescription").unwrap_or("--".to_string());
				format!("S{number}_{desc}")
			},
			Study(_) => {
				let id=self.get_string("PatientID").unwrap_or("--".to_string());
				let date=self.get_string("StudyDate").unwrap_or("--".to_string());
				let time=self.get_string("StudyTime").unwrap_or("--".to_string());
				format!("{id}/{date}_{time}")
			}
		}
	}

	pub fn remove(&mut self,key:&str) -> Option<sql::Value>
	{
		match self {
			Instance((_,data)) | Series((_,data)) | Study((_,data)) => data
		}.remove(key)
	}
}

impl TryFrom<sql::Object> for Entry
{
	type Error = anyhow::Error;

	fn try_from(mut obj: sql::Object) -> std::result::Result<Self, Self::Error>
	{
		match obj.remove("id").ok_or(anyhow!(r#"Entry missing "id""#))?
		{
			sql::Value::Thing(id) => {
				match id.tb.as_str() {
					"instances" => Ok(Instance((id, obj))),
					"series" => Ok(Series((id, obj))),
					"studies" => Ok(Study((id, obj))),
					_ => Err(anyhow!("invalid table name"))
				}
			}
			_ => Err(anyhow!(r#""id" is not an id"#))
		}
	}
}

impl Serialize for Entry
{
	fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> where S: Serializer
	{
		let typename= match self {
			Instance(_) => "instance",
			Series(_) => "series",
			Study(_) => "study"
		};
		let mut map = self.data().1.clone();
		map.insert("id".to_string(),self.id().clone().into());

		let mut s= serializer.serialize_map(Some(1))?;
		s.serialize_entry(typename,&map)?;
		s.end()
	}
}
