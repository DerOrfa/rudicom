use std::fmt::{Display, Formatter};
use std::sync::OnceLock;

use byte_unit::Byte;
use byte_unit::UnitType::Binary;
use serde::de::DeserializeOwned;
use serde::Serialize;
use surrealdb::{Response, sql, Surreal};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::{Thing, Value};

pub use entry::Entry;
pub use file::File;
pub use into_db_value::IntoDbValue;
pub use register::{register_instance, RegistryGuard, unregister};

use crate::tools::{Context, Error, Result};

mod into_db_value;
mod register;
mod entry;
mod file;

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

static DB: OnceLock<Surreal<Any>> = OnceLock::new();

fn db() -> &'static Surreal<Any>
{
	DB.get_or_init(Surreal::init)
}

async fn query(qry:impl IntoQuery, bindings: impl Serialize) -> surrealdb::Result<Value>
{
	db()
		.query(qry)
		.bind(bindings)
		.await?.take::<Value>(0)
}

/// Executes `select {child} as val from $id`
pub async fn list_fields<T>(id: &Thing, child:&str) -> Result<T> where T:DeserializeOwned, T:Default
{
	let res:Option<T>=db()
		.query(format!("select {child} as val from $id PARALLEL"))
		.bind(("id",id)).await
		.and_then(Response::check)
		.and_then(|mut r|r.take("val"))
		.context(format!("querying for {child} from {id}"))?;
	Ok(res.unwrap_or(T::default()))
}

pub(crate) async fn list<'a,T>(table:T,selector: Selector<'a>) -> Result<Vec<Value>> where sql::Table:From<T>
{
	let table:sql::Table = table.into();
	let query_context = format!("querying for {selector} in table {table}");
	let value=query(format!("select {selector} from $table  PARALLEL"), ("table", table)).await.context(&query_context)?;
	match value {
		Value::Array(rows) => Ok(rows.0),
		Value::None => Err(Error::NotFound),
		_ => Err(Error::UnexpectedResult {expected:"list of entries".into(),found:value})
	}.context(query_context)
}
pub(crate) async fn list_refs<'a,T>(id:&Thing, col:T, selector: Selector<'a>, flatten:bool) -> Result<Vec<Value>> where T:AsRef<str>
{
	let query_context = format!("when looking up {} in {}",col.as_ref(),id);
	let mut result = query(format!("select {selector} from $id.{} PARALLEL",col.as_ref()),("id",id)).await
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
pub(crate) async fn list_children<T>(id:&Thing, col:T) -> Result<Vec<Entry>> where T:AsRef<str>
{
	let result:Result<Vec<_>>= list_refs(id, col.as_ref(), Selector::All, true).await?.into_iter()
		.map(Entry::try_from)
		.collect();
	result.context(format!("listing children of {id} in column {}",col.as_ref()))
}

pub(crate) async fn list_json<'a,T>(id:&Thing, selector: Selector<'a>, col:T) -> Result<Vec<serde_json::Value>> where T:AsRef<str>
{
	let list= list_refs(id, col, selector, true).await?;
	Ok(list.into_iter().map(Value::into_json).collect())
}
pub(crate) async fn lookup(id:&Thing) -> Result<Option<Entry>>
{
	query("select * from $id", ("id", id)).await
		.map_err(Error::from)
		.and_then(|value| {
			if let Value::Array(a) = &value {
				if a.is_empty() { return Ok(None) }
			};
			if !value.is_some(){ return Ok(None) }
			Some(Entry::try_from(value)).transpose()
		})
		.context(format!("looking up {id}"))
}

async fn query_for_thing<T>(id:&Thing, col:T) -> Result<Thing> where T:AsRef<str>
{
	let query_context=format!("querying for {} in {id}",col.as_ref());
	let res:Option<Thing> = db()
		.query(format!("select {} from $id",col.as_ref()))
		.bind(("id",id.to_owned())).await
		.and_then(Response::check)
		.and_then(|mut r|r.take(col.as_ref()))
		.context(&query_context)?;
	res.ok_or(Error::NotFound.context(query_context))
}

pub async fn find_down_tree(id:&Thing) -> Result<Vec<Thing>>
{
	let query_context = format!("looking for parents of {id}");
	match id.tb.as_str() {
		"instances" => {
			let series = query_for_thing(id, "series").await.map_err(|e|e.context(&query_context))?;
			let study = query_for_thing(&series, "study").await.map_err(|e|e.context(&query_context))?;
			Ok(vec![id.to_owned(),series,study])
		},
		"series" => {
			let study = query_for_thing(id, "study").await.map_err(|e|e.context(&query_context))?;
			Ok(vec![id.to_owned(), study])
		},
		"studies" => Ok(vec![id.to_owned()]),
		_ => {Err(Error::InvalidTable {table:id.tb.to_string()}.context(query_context))}
	}
}

#[cfg(feature = "embedded")]
pub async fn init_local(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.to_string_lossy()).as_str());
	let file = format!("rocksdb://{file}");
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
	db().query(include_str!("init.surreal")).await?;
	Ok(())
}

pub async fn version() -> surrealdb::Result<String>
{
	Ok(format!("{}",db().version().await?))
}

#[derive(Serialize)]
pub struct Stats
{
	instances:usize,
	size_mb:String,
	db_version:String,
	health:String,
	version:String
}
pub async fn statistics() -> Result<Stats>
{
	let picker=sql::idiom("size").map_err(|e|Error::SurrealError(e.into()))?;
	let instances_v= list("instances",Selector::Select("file.size as size")).await?;
	let instances = instances_v.len();
	let size =	instances_v.into_iter().map(|v|v.pick(&picker))
		.filter_map(|v|if let Value::Number(n)=v{Some(n.as_int() as u64)} else {None})
		.map(Byte::from)
		.reduce(|a,b|a.add(b).unwrap_or(Byte::MAX)).unwrap_or(Byte::MIN);
	let health= match db().health().await{
		Ok(_) => String::from("good"),
		Err(e) => e.to_string()
	};
	
	Ok(Stats{
		instances,health,version:env!("CARGO_PKG_VERSION").to_string(),
		size_mb:format!("{:.2}",size.get_appropriate_unit(Binary)),
		db_version:db().version().await?.to_string()
	})
}

pub fn get_from_object<Q>(obj: &sql::Object, key: Q) -> Result<&Value>
	where String:From<Q>, 
{
	let element = String::from(key);
	obj.0.get(&element)
		.ok_or(Error::ElementMissing {element,parent:"file object".into()})
}
