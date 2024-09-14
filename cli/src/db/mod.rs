use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::sync::OnceLock;

use byte_unit::Byte;
use byte_unit::UnitType::Binary;
use chrono::offset::Utc;
use chrono::{SecondsFormat, TimeDelta};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::{Datetime, Id, Idiom, Value};
use surrealdb::{sql, RecordIdKey, Response, Surreal};

pub use entry::Entry;
pub use file::File;
pub use into_db_value::IntoDbValue;
pub use register::{register_instance, unregister, RegistryGuard};

use crate::tools::{Context, Error, Result};

mod into_db_value;
mod register;
mod entry;
mod file;


#[derive(Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct RecordId(surrealdb::RecordId);

impl RecordId {
	pub(crate) fn instance<I>(id: I) -> RecordId where RecordIdKey: From<I> 
	{
		RecordId(surrealdb::RecordId::from(("instances",id)))
	}
	pub(crate) fn series<I>(id: I) -> RecordId where RecordIdKey: From<I>
	{
		RecordId(surrealdb::RecordId::from(("series",id)))
	}
	pub(crate) fn study<I>(id: I) -> RecordId where RecordIdKey: From<I>
	{
		RecordId(surrealdb::RecordId::from(("studies",id)))
	}
	pub(crate) fn raw_key(&self) -> String {
		if let Id::String(key) = self.key().clone().into_inner() {key} 
		else  {panic!("Only string IDs are allowed")}
	}
}

impl Eq for RecordId {}
impl Display for RecordId {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}

impl Into<Value> for RecordId
{
	fn into(self) -> Value {
		self.0.into_inner().into()
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
			_ => return tb,
		}
	}
}

impl<R> From<R> for RecordId where surrealdb::RecordId: From<R> 
{
	fn from(value: R) -> Self {RecordId(value.into())}
}

pub enum Selector<'a>{
	Select(&'a str),
	All
}

impl<'a> AsRef<str> for Selector<'a>
{
	fn as_ref(&self) -> &str {
		match *self {
			Selector::Select(s) => s,
			Selector::All => "*"
		}
	}
}
impl<'a> Display for Selector<'a>
{
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_ref())
	}
}

#[derive(Deserialize,Debug)]
pub struct InstancesPer
{
	pub count:usize,
	pub size:u64, 
	pub me:RecordId
}

impl InstancesPer 
{
	pub async fn select(id:RecordId) -> Result<Self>
	{
		let res:Option<Self>=db().select(&id.0).await?;
		res.ok_or(Error::ElementMissing {element:id.0.key().to_string(),parent:id.0.table().to_string()})
	}
}

static DB: OnceLock<Surreal<Any>> = OnceLock::new();

fn db() -> &'static Surreal<Any>
{
	DB.get_or_init(Surreal::init)
}

fn make_pick_from_valid(pick:&str) -> Idiom
{
	sql::idiom(pick).expect("should be a valid idiom")
}

async fn query(qry:impl IntoQuery, bindings: impl Serialize+'static) -> surrealdb::Result<Value>
{
	let mut result= db()
		.query(qry)
		.bind(bindings)
		.await?;
	result.take::<surrealdb::Value>(0usize).map(|v|v.into_inner())
}

/// Executes `select {child} as val from $id`
pub async fn list_fields<T>(id: RecordId, child:&str) -> Result<T> where T:DeserializeOwned, T:Default
{
	let ctx = format!("querying for {child} from {id}");
	let res:Option<T>=db()
		.query(format!("select {child} as val from $id PARALLEL"))
		.bind(("id", id.0)).await
		.and_then(Response::check)
		.and_then(|mut r|r.take("val"))
		.context(ctx)?;
	Ok(res.unwrap_or(T::default()))
}

pub(crate) async fn list<'a,T>(table:T,selector: Selector<'a>) -> Result<Vec<sql::Value>> where sql::Table:From<T>
{
	let table:sql::Table = table.into();
	let query_context = format!("querying for {selector} in table {table}");
	let value= query(format!("select {selector} from $table  PARALLEL"), ("table", table)).await.context(&query_context)?;
	match value {
		Value::Array(rows) => Ok(rows.0),
		Value::None => Err(Error::NotFound),
		_ => Err(Error::UnexpectedResult {expected:"list of entries".into(),found:value})
	}.context(query_context)
}
pub(crate) async fn list_refs<'a,T>(id:RecordId, col:T, selector: Selector<'a>, flatten:bool) -> Result<Vec<Value>> where T:AsRef<str>
{
	let query_context = format!("when looking up {} in {}",col.as_ref(),id);
	let mut result = query(
		format!("select {selector} from $id.{} PARALLEL",col.as_ref()),
		("id",id.0)
	).await
		.context(&query_context)?;
	if flatten {result=Value::flatten(result)}
	match result
	{
		Value::Array(values) => Ok(values.0),
		_ => Err(
				Error::UnexpectedResult	{
					expected: String::from("Array"),
					found: result
				}.context(query_context)
		)
	}
}
pub(crate) async fn list_children<T>(id:RecordId, col:T) -> Result<Vec<Entry>> where T:AsRef<str>
{
	let ctx = format!("listing children of {id} in column {}",col.as_ref());
	let result:Result<Vec<_>>= list_refs(id, col.as_ref(), Selector::All, true).await?.into_iter()
		.map(Entry::try_from)
		.collect();
	result.context(ctx)
}

pub(crate) async fn list_json<'a,T>(id:RecordId, selector: Selector<'a>, col:T) -> Result<Vec<serde_json::Value>> where T:AsRef<str>
{
	let list= list_refs(id, col, selector, true).await?;
	Ok(list.into_iter().map(Value::into_json).collect())
}
pub(crate) async fn lookup(id:RecordId) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {id}");
	query("select * from $id", ("id", id.0)).await
		.map_err(Error::from)
		.and_then(|value| {
			if let Value::Array(a) = &value {
				if a.is_empty() { return Ok(None) }
			};
			if !value.is_some(){ return Ok(None) }
			Some(Entry::try_from(value)).transpose()
		})
		.context(ctx)
}

async fn query_for_record<T>(id:RecordId, col:T) -> Result<RecordId> where T:AsRef<str>
{
	let query_context=format!("querying for {} in {id}",col.as_ref());
	let res:Option<RecordId> = db()
		.query(format!("select {} from $id",col.as_ref()))
		.bind(("id",id.0)).await
		.and_then(Response::check)
		.and_then(|mut r|r.take(col.as_ref()))
		.context(&query_context)?;
	res.ok_or(Error::NotFound.context(query_context))
}

pub async fn find_down_tree(id:RecordId) -> Result<Vec<RecordId>>
{
	let query_context = format!("looking for parents of {id}");
	match id.table() {
		"instances" => {
			let series = query_for_record(id.clone(), "series").await.map_err(|e|e.context(&query_context))?;
			let study = query_for_record(series.clone(), "study").await.map_err(|e|e.context(&query_context))?;
			Ok(vec![id,series,study])
		},
		"series" => {
			let study = query_for_record(id.clone(), "study").await.map_err(|e|e.context(&query_context))?;
			Ok(vec![id, study])
		},
		"studies" => Ok(vec![id.to_owned()]),
		_ => {Err(Error::InvalidTable {table:id.table().to_string()}.context(query_context))}
	}
}

pub async fn init_local(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.to_string_lossy()).as_str());
	let file = format!("surrealkv://{file}");
	db().connect(file).await?;
	init().await
}
pub async fn init_remote(addr:&str) -> surrealdb::Result<()>
{
	db().connect(addr).await?;

	// Sign in as a namespace, database, or root user
	db().signin(Root { username: "root", password: "root", }).await?;
	init().await
}
async fn init() -> surrealdb::Result<()>{
	// Select a specific namespace / database
	db().use_ns("namespace").use_db("database").await?;
	db().query(include_str!("init.surql")).await?;
	Ok(())
}

pub async fn version() -> surrealdb::Result<String>
{
	Ok(format!("{}",db().version().await?))
}

pub async fn changes(since:Datetime) -> Result<Vec<Entry>>
{
	let since = since.to_rfc3339_opts(SecondsFormat::Secs, true); 
	let res=db().query(format!(r#"SHOW CHANGES FOR TABLE instances SINCE "{since}" LIMIT 1000"#))
		.await?.take::<surrealdb::Value>(0)?.into_inner();
	match res.pick(&sql::idiom("changes.update").expect("should be a valid idiom")).flatten()
	{
		Value::Array(a) => a.into_iter().map(Entry::try_from).collect(),
		_ => return Err(Error::UnexpectedResult {expected:"array of changes".into(),found:res})
	}
}

#[derive(Serialize)]
pub struct Stats
{
	studies:usize,
	instances:usize,
	size_mb:String,
	db_version:String,
	health:String,
	version:String,
	activity:String
}
pub async fn statistics() -> Result<Stats>
{
	let size_picker=make_pick_from_valid("size");
	let count_picker=make_pick_from_valid("count");
	let studies_v= list("instances_per_studies",Selector::Select("size")).await?;
	let instances= list("instances_per_studies",Selector::Select("count")).await?
		.into_iter().map(move|v|v.pick(&count_picker)).filter_map(|v|sql::Number::try_from(v).ok())
		.map(|n|n.as_usize()).reduce(|a,b|a+b).unwrap_or_default();
	let studies = studies_v.len();
	
	// let instances = instances_v.len();
	let size = studies_v.into_iter().map(|v|v.pick(&size_picker))
		.filter_map(|v|sql::Number::try_from(v).ok()).map(|n|n.as_usize())
		.map(Byte::from)
		.reduce(|a,b|a.add(b).unwrap_or(Byte::MAX)).unwrap_or(Byte::MIN);
	let health= match db().health().await{
		Ok(_) => String::from("good"),
		Err(e) => e.to_string()
	};
	
	let timestamp = Utc::now()-TimeDelta::try_seconds(10).unwrap();
	let changes=changes(timestamp.into()).await?.len();
	
	Ok(Stats{
		studies,instances,health,version:env!("CARGO_PKG_VERSION").to_string(),
		size_mb:format!("{:.2}",size.get_appropriate_unit(Binary)),
		db_version:db().version().await?.to_string(),
		activity:format!("{changes} updates within the last 10 seconds")
	})
}

pub fn get_from_object<Q>(obj: &sql::Object, key: Q) -> Result<&Value>
	where String:From<Q>, 
{
	let element = String::from(key);
	obj.0.get(&element)
		.ok_or(Error::ElementMissing {element,parent:"file object".into()})
}
