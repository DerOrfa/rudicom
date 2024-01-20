use std::collections::BTreeMap;
use std::path::PathBuf;
use surrealdb::sql;
use crate::db;
use crate::db::{DBErr, File};
use crate::tools::transform;
use self::Entry::{Instance, Series, Study};

#[derive(Clone,Debug)]
pub enum Entry
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

	pub fn get_file(&self) -> Result<db::File,DBErr>
	{
		File::try_from(self.clone())
	}
	pub fn get_path(&self) -> Result<PathBuf,DBErr>
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
	type Error = surrealdb::error::Api;

	fn try_from(mut value: sql::Value) -> Result<Self, Self::Error>
	{
		match value {
			sql::Value::Array(ref mut array) => {
				if array.len() == 1 { Entry::try_from(array.0.remove(0)) } else {
					Err(Self::Error::FromValue {
						error:"Array must have exactly one element for conversion to entry".to_string(),
						value:value.to_owned()
					})
				}
			}
			sql::Value::Object(obj) => Entry::try_from(obj),
			_ => Err(Self::Error::FromValue {
				error:"Invalid value to convert into entry".to_string(),
				value
			}),
		}
	}
}
impl TryFrom<sql::Object> for Entry
{
	type Error = surrealdb::error::Api;

	fn try_from(mut obj: sql::Object) -> Result<Self, Self::Error>
	{
		let id_val = obj.remove("id")
			.ok_or(Self::Error::FromValue{
				error:r#"Entry missing "id""#.to_string(),
				value:sql::Value::from(obj.clone())
			})?;
		match id_val
		{
			sql::Value::Thing(id) => {
				match id.tb.as_str() {
					"instances" => Ok(Instance((id, obj))),
					"series" => Ok(Series((id, obj))),
					"studies" => Ok(Study((id, obj))),
					_ => Err(Self::Error::FromValue{
						error:r#"invalid table name"#.to_string(),value:id.into()
					})
				}
			}
			_ => Err(Self::Error::FromValue{
				error:r#"not an id"#.to_string(),value:id_val.into()
			})
		}
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
