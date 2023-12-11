use std::collections::BTreeMap;
use std::path::PathBuf;
use surrealdb::sql;
use anyhow::anyhow;
use serde::{Serialize, Serializer};
use serde::ser::SerializeMap;
use crate::db;
use crate::tools::transform;
use self::Entry::{Instance, Series, Study};

pub enum Entry
{
	Instance((sql::Thing,sql::Object)),
	Series((sql::Thing,sql::Object)),
	Study((sql::Thing,sql::Object))
}

impl Entry
{
	fn type_name(&self) -> &str
	{
		match self {
			Instance(_) => "instance",
			Series(_) => "series",
			Study(_) => "study"
		}
	}
	pub fn get(&self, key:&str) -> Option<&sql::Value>
	{
		self.data().1.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<String>
	{
		self.get(key).map(|v|v.to_raw_string())
	}
	fn data(&self) -> &(sql::Thing,sql::Object)
	{
		match self {
			Instance(data)| Series(data) | Study(data) => data
		}
	}
	fn mut_data(&mut self) -> &mut (sql::Thing,sql::Object)
	{
		match self {
			Instance(data)| Series(data) | Study(data) => data
		}
	}
	pub fn id(&self) -> &sql::Thing	{&self.data().0}

	pub fn name(&self) -> String
	{
		match self {
			Instance(_) => {
				let number=self.get_string("InstanceNumber").unwrap_or("<-->".to_string());
				format!("Instance {number}")
			},
			Series(_) => {
				let number=self.get_string("SeriesNumber").unwrap_or("<-->".to_string());
				let desc= self.get_string("SeriesDescription").unwrap_or("<-->".to_string());
				format!("S{number}_{desc}")
			},
			Study(_) => {
				let id=self.get_string("PatientID").unwrap_or("<-->".to_string());
				let date=self.get_string("StudyDate").unwrap_or("<-->".to_string());
				let time=self.get_string("StudyTime").unwrap_or("<-->".to_string());
				format!("{id}/{date}_{time}")
			}
		}
	}

	pub fn remove(&mut self,key:&str) -> Option<sql::Value>
	{
		self.mut_data().1.remove(key)
	}
	pub fn insert<T,K>(&mut self,key:K,value:T) -> Option<sql::Value> where T:Into<sql::Value>,K:Into<String>
	{
		self.mut_data().1.insert(key.into(),value.into())
	}

	pub fn get_file(&self) -> anyhow::Result<db::File>
	{
		if let Instance((_,o))=self {
			o.get("file")
				.ok_or(anyhow!(r#"Entry is missing "file""#))
				.and_then(|v|db::File::try_from(v.clone()))
		} else {Err(anyhow!("Not an instance"))}
	}
	pub fn get_path(&self) -> anyhow::Result<PathBuf>
	{
		match self {
			Instance(_) =>
				self.get_file().map(|f|f.get_path()),
			Series(_) => {todo!()}
			Study(_) => {todo!()}
		}
	}
}

impl From<Entry> for sql::Object {
	fn from(entry: Entry) -> Self {
		match entry {
			Instance(mut data)| Series(mut data) | Study(mut data) => {
				data.1.insert("id".into(),data.0.into());
				data.1
			}
		}
	}
}
impl From<Entry> for sql::Value {
	fn from(entry: Entry) -> Self {	sql::Object::from(entry).into()	}
}

impl TryFrom<sql::Value> for Entry
{
	type Error = anyhow::Error;

	fn try_from(value: sql::Value) -> Result<Self, Self::Error>
	{
		match value {
			sql::Value::None | sql::Value::Null => Err(anyhow!("value is empty")),
			sql::Value::Array(mut array) => {
				if array.len() == 1 { Entry::try_from(array.remove(0)) }
				else {Err(anyhow!("Exactly one entry was expected"))}

			},
			sql::Value::Object(obj) => Entry::try_from(obj),
			_ => Err(anyhow!("Value {value:?} has invalid form"))
		}
	}
}

impl TryFrom<sql::Object> for Entry
{
	type Error = anyhow::Error;

	fn try_from(mut obj: sql::Object) -> Result<Self, Self::Error>
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
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer
	{
		let typename= self.type_name();
		let mut map = self.data().1.clone();
		map.insert("id".to_string(),self.id().id.to_raw().into());

		let mut s= serializer.serialize_map(Some(1))?;
		s.serialize_entry(typename,&map)?;
		s.end()
	}
}

impl From<Entry> for serde_json::Value
{
	fn from(entry: Entry) -> Self {
		// transform all Thing-objects into generic Objects to make them more useful in json
		let transformer = |v|if let sql::Value::Thing(id)=v{
			sql::Object(BTreeMap::from([
				("tb".into(), id.tb.into()),
				("id".into(), id.id.to_raw().into()),
			])).into()
		} else {v};
		let value = transform(entry.into(),transformer);
		let object = sql::Object::try_from(value).unwrap();
		serde_json::Value::from(sql::Value::Object(object))
	}
}
