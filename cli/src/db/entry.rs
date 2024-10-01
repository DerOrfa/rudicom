use std::collections::BTreeMap;
use std::path::PathBuf;
use byte_unit::Byte;
use surrealdb::{Value,Object};
use crate::db;
use crate::tools::{reduce_path, transform,Result,Context};
use self::Entry::{Instance, Series, Study};

#[derive(Clone,Debug)]
pub enum Entry
{
	Instance((db::RecordId,Object)),
	Series((db::RecordId,Object)),
	Study((db::RecordId,Object))
}

impl Entry
{
	pub fn get(&self, key:&str) -> Option<&Value>
	{
		let obj:&Object = self.as_ref();
		obj.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<String>
	{
		self.get(key).map(|v|v.into_inner_ref().to_raw_string())
	}
	pub fn id(&self) -> &db::RecordId {&self.as_ref()}

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
			Series((id,_)) => db::list_fields(id.clone(), "instances.file").await
				.context(format!("listing files in series {id}")),

			Study((id,_)) => db::list_fields(id.clone(), "array::flatten(series.instances.file)").await
				.context(format!("listing files in study {id}")),
		}
	}
	
	/// summarize size of all files in this entry
	pub async fn size(&self) -> Result<Byte>
	{
		let size:u64=match self {
			Instance(_) => self.get_file().map(|f|f.size),
			Series((id,_)) => 
				db::list_fields(id.clone(), "math::sum(instances.file.size)").await,
			Study((id,_)) => 
				db::list_fields(id.clone(), "math::sum(array::flatten(series.instances.file.size))").await,
		}?;
		Ok(Byte::from(size))
	}

	pub fn remove(&mut self,key:&str) -> Option<Value>
	{
		self.as_mut().remove(key)
	}
	pub fn insert<T,K>(&mut self,key:K,value:T) -> Option<Value> where T:Into<Value>,K:Into<String>
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

impl AsRef<db::RecordId> for Entry
{
	fn as_ref(&self) -> &db::RecordId {
		match self {
			Instance((id,_))| Series((id,_)) | Study((id,_)) => id
		}
	}
}

impl AsRef<Object> for Entry
{
	fn as_ref(&self) -> &Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &data.1
		}
	}
}
impl AsMut<Object> for Entry
{
	fn as_mut(&mut self) -> &mut Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &mut data.1
		}
	}
}

impl From<Entry> for Object {
	fn from(entry: Entry) -> Self {
		match entry {
			Instance(mut data)| Series(mut data) | Study(mut data) => {
				data.1.insert("id".into(),data.0.0);
				data.1
			}
		}
	}
}
impl From<Entry> for Value {
	fn from(entry: Entry) -> Self {	
		Value::from_inner(Object::from(entry).into_inner().into())	
	}
}

impl TryFrom<Value> for Entry
{
	type Error = crate::tools::Error;

	fn try_from(value: Value) -> std::result::Result<Self, Self::Error>
	{
		let kind = value.into_inner_ref().kindof();
		let err = Self::Error::UnexpectedResult {expected:"single object".into(),found:kind};
		match value.into_inner() {
			surrealdb::sql::Value::Array(ref mut array) => {
				if array.len() == 1 {
					let last = array.drain(..1).last().unwrap();
					Entry::try_from(Value::from_inner(last)) 
				} else {
					Err(err)
				}
			}
			surrealdb::sql::Value::Object(obj) => Entry::try_from(surrealdb::Object::from_inner(obj)),
			_ => Err(err),
		}.context("trying to convert database value into an Entry")
	}
}
impl TryFrom<Object> for Entry
{
	type Error = crate::tools::Error;

	fn try_from(mut obj: Object) -> std::result::Result<Self, Self::Error>
	{
		obj.remove("id")
			.ok_or(Self::Error::ElementMissing{element:"id".into(),parent:obj.to_string()})
			.map(Value::into_inner)
			.and_then(|id_val|
				match id_val
				{
					surrealdb::sql::Value::Thing(id) => 
					{
						match id.tb.as_str() {
							"instances" | "series" | "studies" => Ok(Study((surrealdb::RecordId::from_inner(id).into(), obj))),
							_ => Err(Self::Error::InvalidTable{table:id.tb})
						}
					}
					_ => Err(Self::Error::UnexpectedResult{expected:"id".into(),found:id_val.kindof()})
				}
			).context("trying to convert database object into an Entry")
	}
}

impl From<Entry> for serde_json::Value
{
	fn from(entry: Entry) -> Self {
		// transform all Thing-objects into generic Objects to make them more useful in json
		let transformer = |v|if let surrealdb::sql::Value::Thing(id)=v
		{
			surrealdb::sql::Object::from(BTreeMap::from([
				("tb", id.tb.into()),
				("id", id.id.to_raw().into()),
			])).into()
		} else {v};
		transform(surrealdb::Value::from(entry).into_inner(),transformer).into()
	}
}
