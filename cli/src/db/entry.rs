use std::collections::BTreeMap;
use std::path::PathBuf;

use byte_unit::Byte;
use surrealdb::sql;

use crate::db;
use crate::tools::{reduce_path, transform};
use crate::tools::{Context, Result};

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
		let obj:&sql::Object = self.as_ref();
		obj.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<String>
	{
		self.get(key).map(sql::Value::to_raw_string)
	}
	pub fn id(&self) -> &sql::Thing	{&self.as_ref()}

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
				let mut date=self.get_string("StudyDate").unwrap_or("<-->".to_string());
				let mut time=self.get_string("StudyTime").unwrap_or("<-->".to_string());
				if date.len()>6 {date=date.split_off(2);}
				time.truncate(6);
				format!("{id}/{date}_{time}")
			}
		}
	}
	
	/// list all file objects in this entry
	pub async fn get_files(&self) -> Result<Vec<db::File>>
	{
		match self {
			Instance(_) => {self.get_file().map(|f|vec![f])},
			Series((id,_)) => db::list_fields(&id, "instances.file").await
				.context(format!("listing files in series {id}")),

			Study((id,_)) => db::list_fields(&id, "array::flatten(series.instances.file)").await
				.context(format!("listing files in study {id}")),
		}
	}
	
	/// summarize size of all files in this entry
	pub fn size(&self) -> Option<Byte>
	{
		if let Instance(_)=self {
			return self.get_file().map(|f|Byte::from(f.size)).ok()
		}
		self.get("size")
			.map(|v|sql::Number::try_from(v).unwrap_or_default())
			.map(|n|Byte::from(n.as_usize()))
	}

	pub fn remove(&mut self,key:&str) -> Option<sql::Value>
	{
		self.as_mut().remove(key)
	}
	pub fn insert<T,K>(&mut self,key:K,value:T) -> Option<sql::Value> where T:Into<sql::Value>,K:Into<String>
	{
		self.as_mut().insert(key.into(),value.into())
	}

	pub fn get_file(&self) -> Result<db::File>
	{
		db::File::try_from(self.clone())
	}
	pub async fn get_path(&self) -> Result<PathBuf>
	{
		// get all files in the entry
		let files=self.get_files().await?;
		// makes PathBuf of them
		Ok(reduce_path(files.iter().map(db::File::get_path).collect()))
	}
}

impl AsRef<sql::Thing> for Entry
{
	fn as_ref(&self) -> &sql::Thing {
		match self {
			Instance(data)| Series(data) | Study(data) => &data.0
		}
	}
}

impl AsRef<sql::Object> for Entry
{
	fn as_ref(&self) -> &sql::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &data.1
		}
	}
}
impl AsMut<sql::Object> for Entry
{
	fn as_mut(&mut self) -> &mut sql::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &mut data.1
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
	type Error = crate::tools::Error;

	fn try_from(mut value: sql::Value) -> std::result::Result<Self, Self::Error>
	{
		match value {
			sql::Value::Array(ref mut array) => {
				if array.len() == 1 { Entry::try_from(array.0.remove(0)) } else {
					Err(Self::Error::UnexpectedResult {expected:"single object".into(),found:value.to_owned()})
				}
			}
			sql::Value::Object(obj) => Entry::try_from(obj),
			_ => Err(Self::Error::UnexpectedResult {expected:"single object".into(),found:value}),
		}.context("trying to convert database value into an Entry")
	}
}
impl TryFrom<sql::Object> for Entry
{
	type Error = crate::tools::Error;

	fn try_from(mut obj: sql::Object) -> std::result::Result<Self, Self::Error>
	{
		obj.remove("id")
			.ok_or(Self::Error::ElementMissing{element:"id".into(),parent:obj.to_string()})
			.and_then(|id_val|
				match id_val
				{
					sql::Value::Thing(id) => {
						match id.tb.as_str() {
							"instances" => Ok(Instance((id, obj))),
							"series" => Ok(Series((id, obj))),
							"studies" => Ok(Study((id, obj))),
							_ => Err(Self::Error::InvalidTable{table:id.tb})
						}
					}
					_ => Err(Self::Error::UnexpectedResult{expected:"id".into(),found:id_val.into()})
				}
			).context("trying to convert database object into an Entry")
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
