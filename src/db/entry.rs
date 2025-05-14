use std::ops::Deref;
use self::Entry::{Instance, Series, Study};
use crate::db;
use crate::db::{AggregateData, Pickable, RecordId, DB};
use crate::tools::Error::{NotFound, UnexpectedResult};
use crate::tools::{entries_for_record, reduce_path, Context, Result};
use byte_unit::Byte;
use std::path::PathBuf;
use dicom::object::DefaultDicomObject;
use surrealdb::sql;
use crate::dcm::{extract, INSTANCE_TAGS, SERIES_TAGS, STUDY_TAGS};

#[derive(Clone,Debug)]
pub enum Entry
{
	Instance((RecordId,surrealdb::Object)),
	Series((RecordId,surrealdb::Object)),
	Study((RecordId,surrealdb::Object))
}

impl Entry
{
	pub fn get(&self, key:&str) -> Option<&surrealdb::Value>
	{
		let obj:&surrealdb::Object = self.as_ref();
		obj.get(key)
	}
	pub fn get_string(&self, key:&str) -> Option<String>
	{
		self.get(key).map(|v|v.into_inner_ref().to_raw_string())
	}
	pub fn id(&self) -> &RecordId {&self.as_ref()}
	
	pub async fn get_instances_per(&self) -> Result<AggregateData>
	{
		let res:Option<AggregateData>=DB.select(self.id().to_aggregate()).await?;
		res.ok_or(NotFound)
	}

	pub fn name(&self) -> String
	{
		match self {
			Instance(_) => {
				let number=self.get_string("Number").unwrap_or("<-->".to_string());
				format!("Instance {number}")
			},
			Series(_) => {
				let number=self.get_string("Number").unwrap_or("<-->".to_string());
				let desc= self.get_string("Description").unwrap_or("<-->".to_string());
				format!("S{number}_{desc}")
			},
			Study(_) => {
				let id=self.get_string("Name").unwrap_or("<-->".to_string());
				let mut date=self.get_string("Date").unwrap_or("<-->".to_string());
				let mut time=self.get_string("Time").unwrap_or("<-->".to_string());
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
				self.get_instances_per().await.context(ctx.as_str())
					.map(|d|Byte::from(d.size))
			}
		}
	}

	pub fn remove(&mut self,key:&str) -> Option<surrealdb::Value>
	{
		self.as_mut().remove(key)
	}
	pub fn insert<T,K>(&mut self,key:K,value:T) -> Option<surrealdb::Value> where T:Into<sql::Value>,K:Into<String>
	{
		self.as_mut().insert(key.into(),surrealdb::Value::from_inner(value.into()))
	}

	pub fn get_file(&self) -> Result<db::File>
	{
		let context= format!("trying to extract a File object from {}",self.id());
		let result = if let Instance((_,inst)) = &self
		{
			inst.pick_ref("file")?.clone().try_into()
		} else {Err(db::Error::UnexpectedEntry {expected:"instance".into(),id:self.id().clone()})};
		result.context(context)
	}
	pub async fn get_path(&self) -> Result<PathBuf>
	{
		// get all files in the entry
		let files=self.get_files().await?;
		// makes PathBuf of them
		if files.is_empty()	{Ok(PathBuf::default())} 
		else { Ok(reduce_path(files.iter().map(db::File::get_path).collect())) }
		
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

impl AsRef<surrealdb::Object> for Entry
{
	fn as_ref(&self) -> &surrealdb::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &data.1
		}
	}
}
impl AsMut<surrealdb::Object> for Entry
{
	fn as_mut(&mut self) -> &mut surrealdb::Object {
		match self {
			Instance(data)| Series(data) | Study(data) => &mut data.1
		}
	}
}

impl From<Entry> for surrealdb::Object {
	fn from(entry: Entry) -> Self {
		match entry {
			Instance(mut data)| Series(mut data) | Study(mut data) => {
				data.1.insert("id".into(),data.0.0);
				data.1
			}
		}
	}
}
impl From<Entry> for surrealdb::Value {
	fn from(entry: Entry) -> Self {
		surrealdb::Value::from_inner(surrealdb::Object::from(entry).into_inner().into())	
	}
}

impl TryFrom<surrealdb::Value> for Entry
{
	type Error = crate::tools::Error;

	fn try_from(value: surrealdb::Value) -> std::result::Result<Self, Self::Error>
	{
		let value = value.into_inner();
		let kind = value.kindof();
		let err = UnexpectedResult {expected:"single object".into(),found:kind};
		match value {
			sql::Value::Array(mut array) => { //@todo probably unnecessary
				if array.len() == 1 {
					let last = array.drain(..1).last().unwrap();
					Entry::try_from(surrealdb::Value::from_inner(last)) 
				} else {
					Err(err)
				}
			}
			sql::Value::Object(obj) => 
				Entry::try_from(surrealdb::Object::from_inner(obj)),
			_ => Err(err),
		}.context("trying to convert database value into an Entry")
	}
}
impl TryFrom<surrealdb::Object> for Entry
{
	type Error = crate::tools::Error;

	fn try_from(mut obj: surrealdb::Object) -> std::result::Result<Self, Self::Error>
	{
		let ctx = "trying to convert database object into an Entry";
		let id = obj.remove("id")
			.ok_or(Self::Error::ElementMissing{element:"id".into(),parent:obj.to_string()})
			.map(surrealdb::Value::into_inner).context(ctx)?;
		match id
		{
			sql::Value::Thing(id) => 
			{
				let id = RecordId(surrealdb::RecordId::from_inner(id));
				match id.table() {
					"instances" => Ok(Instance((id, obj))),
					"series" => Ok(Series((id, obj))),
					"studies" => Ok(Study((id, obj))),
					_ => Err(Self::Error::InvalidTable{table:id.table().to_string()})
				}
			}
			_ => Err(UnexpectedResult{expected:"id".into(),found:id.kindof()})
		}.context(ctx)
	}
}

impl From<Entry> for serde_json::Value
{
	fn from(entry: Entry) -> Self {
		let obj = surrealdb::Object::from(entry);
		crate::tools::conv::value_to_json(obj.into_inner().into()).into()
	}
}
