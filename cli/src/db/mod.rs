use crate::tools::Error::{ElementMissing, UnexpectedResult};
use crate::tools::{Context, Error, Result};
use base64::{engine::general_purpose, Engine as _};
use byte_unit::Byte;
use byte_unit::UnitType::Binary;
pub use entry::Entry;
pub use file::File;
pub use into_db_value::IntoDbValue;
pub use register::{register_instance, unregister, RegistryGuard};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::sync::LazyLock;
use std::vec::IntoIter;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::opt::{IntoQuery, Resource};
use surrealdb::sql::Id;
use surrealdb::{sql, RecordIdKey, Surreal};
use surrealdb::{Object, Value};

mod into_db_value;
mod register;
mod entry;
mod file;


#[derive(Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct RecordId(pub surrealdb::RecordId);

impl RecordId {
	fn str_to_vec(s: &str) -> IntoIter<i64>
	{
		let s = format!("{s:+<64}").replace(".","+");
		let bytes = general_purpose::STANDARD.decode(s).unwrap();
		let mut bytes = bytes.as_slice();
		let mut big:Vec<i64>=vec![];
		while !bytes.is_empty() {
			let (head,rest) = bytes.split_at(size_of::<i64>());
			bytes = rest;
			big.push(i64::from_ne_bytes(head.try_into().unwrap()));
		}
		big.into_iter()
	}

	pub(crate) fn from_instance(instance_id: &str, series_id: &str, study_id: &str ) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.chain(Self::str_to_vec(instance_id))
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("instances",index)))
	}
	pub(crate) fn from_series(series_id: &str, study_id: &str) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("series",index)))
	}
	pub(crate) fn from_study(id: &str) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(id)
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("studies",index)))
	}
	pub(crate) fn str_key(&self) -> String {
		let bytes= self.key_vec();
		// the last 6 numbers in the vector are the actual ID (not parents)
		let bytes:Vec<_> = bytes.split_at(bytes.len()-6).1.to_vec().into_iter()
			.map(i64::try_from)
			.map(|v|v.unwrap().to_ne_bytes())
			.flatten().collect();
		general_purpose::STANDARD.encode(bytes)
			.trim_end_matches("+").to_string()
			.replace("+",".")
	}
	pub(crate) fn key_vec(&self) -> &[sql::Value] {
		if let Id::Array(key) = self.deref().key().into_inner_ref() {
			if key.0.len() == 1 { //aggregate ids are arrays of arrays, just flatten that
				if let sql::Value::Array(array)= &key.0[0]{
					return array.0.as_slice()
				}
			}
			key.0.as_slice()
		}
		else  {panic!("Only vector IDs are allowed")}
	}
	// pub(crate) fn key(&self) -> RecordIdKey {
	// 	let key:Vec<_> = self.key_vec().into_iter()
	// 		.map(|v|v.clone())
	// 		.map(Value::from_inner)
	// 		.collect();
	// 	RecordIdKey::from(key)
	// }

	pub fn to_aggregate(&self) -> surrealdb::RecordId {
		let me = Value::from(self.0.clone());
		match self.table() {
			"series" => surrealdb::RecordId::from_table_key("instances_per_series", vec![me]),
			"studies" => surrealdb::RecordId::from(("instances_per_studies",vec![me])),
			_ => panic!("cannot get aggregate data for {}:{}",self.table(),self.str_key())
		}
	}
}

impl Eq for RecordId {}
impl Display for RecordId {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_fmt(format_args!("{}:{}",self.table(),self.str_key()))
	}
}

impl Into<Value> for RecordId
{
	fn into(self) -> Value {
		surrealdb::Value::from(self.0)
	}
}

impl Deref for RecordId {
	type Target = surrealdb::RecordId;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl Ord for RecordId {
	fn cmp(&self, other: &Self) -> Ordering {
		let tb=self.0.table().cmp(&other.0.table());
		match tb {
			Ordering::Equal => self.0.key().partial_cmp(&other.0.key()).expect("Failed to compare record keys"),
			_ => tb,
		}
	}
}

impl<S> From<(S,Vec<Value>)> for RecordId where S: Into<String>
{
	fn from(value: (S, Vec<Value>)) -> Self {
		let (table,key) = value;
		RecordId(surrealdb::RecordId::from_table_key(table,key))
	}
}

#[derive(Deserialize,Debug)]
pub struct AggregateData
{
	id:surrealdb::RecordId,
	pub count:usize,
	pub size:u64, 
}

impl AggregateData
{
	pub fn get_inner_id(&self) -> RecordId
	{
		let ret = if let Id::Array(array) = self.id.key().into_inner_ref()
		{
			let inner_id_value= array.get(0)
				.expect("aggregate RecordIdKeys must be arrays with one element");
			if let sql::Value::Thing(id) = inner_id_value {
				let inner_key = RecordIdKey::try_from(Value::from_inner(id.clone().id.into()))
					.expect("aggregate RecordIdKeys must be arrays of RecordIds");
				surrealdb::RecordId::from_table_key(id.tb.clone(),inner_key)
			} else {panic!("aggregate RecordIdKeys must be arrays of RecordIds")}
		} else {panic!("aggregate RecordIdKeys must be arrays")};
		RecordId(ret)
	}
}

pub(crate) static DB: LazyLock<Surreal<Any>> = LazyLock::new(Surreal::init);

async fn query(qry:impl IntoQuery, bindings: impl Serialize+'static) -> surrealdb::Result<Value>
{
	let mut result= DB
		.query(qry)
		.bind(bindings)
		.await?;
	result.take::<Value>(0usize)
}

pub(crate) async fn list_entries<T>(table:T) -> Result<Vec<Entry>> where Resource: From<T>
{
	let val = DB.select::<Value>(Resource::from(table)).await?.into_inner();
	let kind = val.kindof();
	if let sql::Value::Array(array) = val {
		array.0.into_iter().map(Value::from_inner).map(Entry::try_from).collect()
	} else {
		Err(UnexpectedResult {found:kind,expected:"list of entries".into()})
	}
}

pub(crate) async fn lookup(id:RecordId) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {id}");
	let v:Value = DB.select(surrealdb::opt::Resource::from(id.0)).await.context(ctx.clone())?;
	if v.into_inner_ref().is_some(){
		Some(Entry::try_from(v)).transpose().context(ctx)
	} else {
		Ok(None)
	}
}

pub(crate) async fn lookup_uid<S:AsRef<str>>(table:S, uid:String) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {uid} in {}",table.as_ref());
	let value= query(format!("select * from {} where uid == $uid",table.as_ref()), ("uid", uid))
		.await.context(ctx.clone())?;
	if value.into_inner_ref().is_truthy() {
		Some(Entry::try_from(value)).transpose().context(ctx)
	}else{
		Ok(None)
	}

}

/// returns [me,parent,parents_parent]
pub fn find_down_tree(id:RecordId) -> Result<Vec<RecordId>>
{
	let query_context = format!("looking for parents of {id}");
	let key_vec:Vec<_> = id.key_vec().to_vec().into_iter().map(Value::from_inner).collect();
	match id.table() {
		"instances" => {
			let (study,_) = key_vec.split_at(6); // just study
			let (series, _) = key_vec.split_at(12); // study + series
			Ok(vec![
				id,
				("series",series.to_vec()).into(),
				("studies",study.to_vec()).into()
			])
		},
		"series" => {
			let (study,_) = key_vec.split_at(6);
			Ok(vec![
				id,("studies",study.to_vec()).into()
			])
		},
		"studies" => Ok(vec![id]),
		_ => {Err(Error::InvalidTable {table:id.table().to_string()}.context(query_context))}
	}
}

pub async fn init_local(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.to_string_lossy()).as_str());
	let file = format!("surrealkv://{file}");
	DB.connect(file).await?;
	init().await
}
pub async fn init_remote(addr:&str) -> surrealdb::Result<()>
{
	DB.connect(addr).await?;

	// Sign in as a namespace, database, or root user
	DB.signin(Root { username: "root", password: "root", }).await?;
	init().await
}
async fn init() -> surrealdb::Result<()>{
	// Select a specific namespace / database
	DB.use_ns("namespace").use_db("database").await?;
	DB.query(include_str!("init.surql")).await?;
	Ok(())
}

#[derive(Serialize)]
pub struct Stats
{
	studies:usize,
	instances:usize,
	stored_size:String,
	db_version:String,
	health:String,
	version:String,
}
pub async fn statistics() -> Result<Stats>
{
	let studies_v:Vec<AggregateData> = DB.select("instances_per_studies").await?;

	let size = studies_v.iter()
		.map(|v|Byte::from(v.size))
		.reduce(|a,b|a.add(b).unwrap_or(Byte::MAX)).unwrap_or(Byte::MIN);
	let instances = studies_v.iter()
		.map(|v|v.count)
		.reduce(|a,b|a+b).unwrap_or(0);
	let studies = studies_v.len();
	
	// let instances = instances_v.len();
	let health= match DB.health().await{
		Ok(_) => String::from("good"),
		Err(e) => e.to_string()
	};
	
	Ok(Stats{
		studies,instances,health,version:env!("CARGO_PKG_VERSION").to_string(),
		stored_size:format!("{:.2}",size.get_appropriate_unit(Binary)),
		db_version:DB.version().await?.to_string(),
	})
}

trait Pickable
{
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>;
	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q>;
}

impl Pickable for Value {
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>{
		let slf = self.into_inner_ref();
		let kind = slf.kindof();
		match slf {
			sql::Value::Object(obj) => Object::from_inner_ref(obj).pick_ref(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let slf = self.into_inner_mut();
		let kind = slf.kindof();
		match slf {
			sql::Value::Object(obj) => Object::from_inner_mut(obj).pick_remove(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}
}

impl Pickable for Object {
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>{
		let element = String::from(element);
		let slf = self.into_inner_ref();
		slf.get(&element).map(Value::from_inner_ref)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let element = String::from(element);
		self.into_inner_mut().remove(&element).map(Value::from_inner)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}
}
