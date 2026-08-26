use self::Entry::{Instance, Series, Study};
use crate::db::{self, AggregateData, DB, Pickable, RecordId};
use crate::dcm::{INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS, extract};
use crate::tools::Error::NotFound;
use crate::tools::{Context, Result, entries_for_record, reduce_path};
use byte_unit::Byte;
use dicom::object::DefaultDicomObject;
use std::ops::Deref;
use std::path::PathBuf;
use surrealdb::types as db_types;
use surrealdb::types::{Kind, SerializationError, SurrealValue, ToSql};

#[derive(Clone, Debug)]
pub enum Entry
{
	Instance((RecordId, db_types::Object)),
	Series((RecordId,db_types::Object)),
	Study((RecordId,db_types::Object))
}

impl Entry
{
	pub fn get(&self, key:&str) -> Option<&db_types::Value>
	{
		let obj:&db_types::Object = self.as_ref();
		obj.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<&str>
	{
		self.get(key).and_then(|v|v.as_string()).map(|s| s.as_str())
	}
	pub fn id(&self) -> &RecordId {&self.as_ref()}

	pub async fn get_aggregate(&self) -> Result<AggregateData>
	{
		match self {
			Instance((_,_)) => {
				let file = self.get_file()?;
				Ok(AggregateData{
					id: self.id().0.clone(),
					count: 1,
					size: file.size,
				})
			}
			Series((_,_)) => {
				let res:Option<AggregateData>=DB.query("select id,math::sum(instances.file.size) as size, count(instances) as count from $rec")
					.bind(("rec", self.id().0.clone()))
					.await?.take(0)?;
				res.ok_or(NotFound)
			}
			Study(_) => {
				let res:Option<AggregateData>=DB.query("select id, count(array::flatten(series.instances)) as count, math::sum(array::flatten(series.instances.file.size)) as size from $rec")
					.bind(("rec", self.id().0.clone()))
					.await?.take(0)?;
				res.ok_or(NotFound)
			}
		}
	}

	pub fn name(&self) -> String
	{
		match self {
			Instance(_) => {
				let number=self.get_string("Number").unwrap_or("<-->");
				format!("Instance {number}")
			},
			Series(_) => {
				let number=self.get_string("Number").unwrap_or("<-->");
				let desc= self.get_string("Description").unwrap_or("<-->");
				format!("S{number}_{desc}")
			},
			Study(_) => {
				let id=self.get_string("Name").unwrap_or("<-->");
				let mut date=self.get_string("Date").unwrap_or("<-->");
				let mut time=self.get_string("Time").unwrap_or("<-->");
				if date.len()>6 {date=&date[2..];}
				if time.len()>6 {time=&time[..6]};
				format!("{id}/{date}_{time}")
			}
		}
	}
	
	/// list all file objects in this entry
	pub async fn get_files(&self) -> Result<Vec<db::FileInfo>>
	{
		match self {
			Instance(_) => {self.get_file().map(|f|vec![f])},
			Series((id,_)) | Study((id,_)) =>{
				entries_for_record(id,"instances").await?
					.iter().map(|e|e.get_file()).collect()
			}
		}
	}
	
	/// summarize size of all files in this entry
	pub async fn size(&self) -> Result<Byte>
	{
		match self {
			Instance(_) => self.get_file().map(|f|Byte::from(f.size)),
			Series(_) | Study(_) => {
				let ctx = format!("extracting size of {}",self.id().str_key());
				Context::context(self.get_aggregate().await, ctx.as_str())
					.map(|d|Byte::from(d.size))
			}
		}
	}

	pub fn remove(&mut self,key:&str) -> Option<db_types::Value>
	{
		self.as_mut().remove(key)
	}
	pub fn insert<T,K>(&mut self,key:K,value:T) -> Option<db_types::Value> where T:Into<db_types::Value>,K:Into<String>
	{
		self.as_mut().insert(key.into(),value.into())
	}

	pub fn get_file(&self) -> Result<db::FileInfo>
	{
		let context= format!("trying to extract a File object from {}",self.id());
		let result = if let Instance((_,inst)) = &self
		{
			inst.pick_ref("file")?.clone().try_into()
		} else {Err(db::Error::UnexpectedEntry {expected:"instance".into(),id:self.id().clone()})};
		Context::context(result,context)
	}
	pub async fn get_path(&self) -> Result<PathBuf>
	{
		// get all files in the entry
		let files=self.get_files().await?;
		// makes PathBuf of them
		if files.is_empty()	{Ok(PathBuf::default())} 
		else { Ok(reduce_path(files.iter().map(db::FileInfo::get_path).collect())) }
		
	}
	 /// Compare the Entry to a Dicom object.
	 /// 
	 /// The list and mapping of relevant Tags is taken from the configuration.
	 /// Same as they would for registration.
	 /// Only values found in the Entry are compared, None is returned for those that don't.  
	 /// 
	 /// # Arguments 
	 /// 
	 /// * `obj`: dicom object to compare against
	 /// 
	 /// returns: Vec<Option<bool>, Global>
	fn compare(&self, obj: &DefaultDicomObject) -> Vec<Option<bool>> {
		let tags = match self {
			Instance(_) => &INSTANCE_TAGS,
			Series(_) => &SERIES_TAGS,
			Study(_) => &STUDY_TAGS
		}.deref();
		extract(obj,tags).into_iter().map(|(key,obj_val)| {
			self.get(key).map(|entry_val|*entry_val==obj_val)
		}).collect()
	}
}

impl SurrealValue for Entry {
	fn kind_of() -> Kind {Kind::Object}

	fn is_value(value: &db_types::Value) -> bool {
		if let Some(obj) = value.as_object() {
			obj.contains_key("id")
		} else { false }
	}

	fn into_value(self) -> db_types::Value {db_types::Value::Object(self.into())}

	fn from_value(value: db_types::Value) -> std::result::Result<Self, db_types::Error> where Self: Sized
	{
		Entry::try_from(value.into_object()?)
	}
}

impl PartialEq<DefaultDicomObject> for Entry {
	fn eq(&self, obj: &DefaultDicomObject) -> bool {
		self.compare(obj).into_iter()
			.filter(Option::is_some) //only values in Entry are considered
			.any(Option::unwrap)
	}
}

impl AsRef<RecordId> for Entry
{
	fn as_ref(&self) -> &RecordId {
		match self {
			Instance((id,_))| Series((id,_)) | Study((id,_)) => id
		}
	}
}

impl AsRef<db_types::Object> for Entry
{
	fn as_ref(&self) -> &db_types::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &data.1
		}
	}
}
impl AsMut<db_types::Object> for Entry
{
	fn as_mut(&mut self) -> &mut db_types::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &mut data.1
		}
	}
}

impl From<Entry> for db_types::Object {
	fn from(entry: Entry) -> Self {
		match entry {
			Instance(mut data)| Series(mut data) | Study(mut data) => {
				data.1.insert("id",data.0.0);
				data.1
			}
		}
	}
}

impl TryFrom<db_types::Object> for Entry
{
	type Error = db_types::Error;

	fn try_from(mut obj: db_types::Object) -> std::result::Result<Self, Self::Error>
	{
		let id = obj.remove("id")
			.ok_or(Self::Error::serialization(
				format!("'id' is missing in '{}'",obj.to_sql_pretty()),
				SerializationError::Deserialization
			))?;
		match id
		{
			db_types::Value::RecordId(id) =>
				match id.table.as_str() {
					"instances" => Ok(Instance((RecordId(id), obj))),
					"series" => Ok(Series((RecordId(id), obj))),
					"studies" => Ok(Study((RecordId(id), obj))),
					_ => Err(Self::Error::serialization(
						format!("Invalid table {}",id.table.to_string()),
							 SerializationError::Deserialization
					)),
				},
			_ => Err(Self::Error::serialization(
				format!("'id' should be a RecordID, but it is '{}'", id.kind().to_string()),
				SerializationError::Deserialization
			))
		}
	}
}

impl From<Entry> for serde_json::Value
{
	fn from(entry: Entry) -> Self {
		let obj = db_types::Object::from(entry);
		crate::tools::conv::value_to_json(db_types::Value::Object(obj)).into()
	}
}

impl Pickable for Entry {
	fn pick_ref<Q>(&self, element: Q) -> Result<&db_types::Value>	where String: From<Q>
	{
		let obj:&db_types::Object = self.as_ref();
		obj.pick_ref(element)
	}

	fn pick_remove<Q>(&mut self, element: Q) -> Result<db_types::Value> where String: From<Q>
	{
		self.as_mut().pick_remove(element)
	}
}