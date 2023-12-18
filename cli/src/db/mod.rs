use std::sync::OnceLock;
use serde::Serialize;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::{Surreal, sql, Response};
use surrealdb::opt::IntoQuery;
use surrealdb::sql::{Value, Thing};
use thiserror::Error;

mod into_db_value;
mod register;
mod entry;
mod file;

pub use into_db_value::IntoDbValue;
pub use register::{unregister, register_instance, RegistryGuard};
pub use entry::Entry;
pub use file::File;

#[derive(Error,Debug)]
pub enum DBErr
{
	#[error("{source} when {context}")]
	Context{
		source:Box<DBErr>,
		context:String
	},
	#[error("Database error {0} when")]
	SurrealError(#[from] surrealdb::Error),
	#[error("Invalid data type (expected {expected:?}, found {found:?})")]
	UnexpectedResult{
		expected: String,
		found: Value,
	},
	#[error("Invalid table {table}")]
	InvalidTable{table:String},
	#[error("No data found")]
	NotFound
}

impl DBErr{
	fn context<T>(self,context:T) -> DBErr where String:From<T>
	{
		DBErr::Context {source:Box::new(self),context:context.into()}
	}
}

pub type Result<T> = std::result::Result<T,DBErr>;

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

pub async fn query_for_list(id: &Thing, target:&str) -> Result<Vec<Thing>>
{
	let res:Option<Vec<Thing>>=db()
		.query(format!("select array::flatten({target}) as id from $id"))
		.bind(("id",id)).await
		.and_then(Response::check)
		.and_then(|mut r|r.take("id"))
		.map_err(|e|DBErr::from(e).context(format!("querying for {target} from {id}")))?;
	Ok(res.unwrap_or(Vec::new()))
}

pub(crate) async fn list_table<T>(table:T) -> Result<Vec<Entry>> where sql::Table:From<T>
{
	let table:sql::Table = table.into();
	let query_context = format!("querying for contents of table {table}");
	let value=query("select * from $table", ("table", table)).await
		.map_err(|e|DBErr::from(e).context(&query_context))?;
	let result:surrealdb::Result<Vec<_>> = match value {
		Value::Array(rows) => {
			rows.0.into_iter()
				.map(|v|Entry::try_from(v).map_err(surrealdb::Error::from))
				.collect()
		},
		Value::None => Err(surrealdb::error::Db::NoRecordFound.into()),
		_ => Err(surrealdb::error::Db::InvalidContent { value }.into())
	};
	result.map_err(|e|DBErr::from(e).context(query_context))
}

pub(crate) async fn list_values<T>(id:&Thing, col:T, flatten:bool) -> Result<Vec<Value>> where T:AsRef<str>
{
	let query_context = format!("when looking up {} in {}",col.as_ref(),id);
	let mut result = query(format!("select * from $id.{}",col.as_ref()),("id",id)).await
		.map_err(|e|DBErr::from(e).context(&query_context))?;
	if flatten {result=Value::flatten(result)}
	match result
	{
		Value::Array(values) => Ok(values.0),
		_ => Err(
				DBErr::UnexpectedResult	{
					expected: String::from("Array"),
					found: result
				}.context(query_context)
		)
	}
}
pub(crate) async fn list_children<T>(id:&Thing, col:T) -> Result<Vec<Entry>> where T:AsRef<str>
{
	let result:Result<Vec<_>>=list_values(id, col.as_ref(),true).await?.into_iter()
		.map(|v|Entry::try_from(v).map_err(surrealdb::Error::Api))
		.map(|r|r.map_err(DBErr::from))
		.collect();
	result.map_err(|e|e.context(format!("listing children of {id} in column {}",col.as_ref())))
}

pub(crate) async fn list_json<T>(id:&Thing, col:T) -> Result<Vec<serde_json::Value>> where T:AsRef<str>
{
	let list=list_values(id, col,true).await?;
	Ok(list.into_iter().map(Value::into_json).collect())
}
pub(crate) async fn lookup(id:&Thing) -> Result<Option<Entry>>
{
	let result = query("select * from $id", ("id", id)).await
		.and_then(|value|
			if value.is_some() {
				Entry::try_from(value).map(|e|Some(e)).map_err(surrealdb::Error::Api)
			} else {
				Ok(None)
			}
		);
	result.map_err(|e|DBErr::from(e).context(format!("when looking up {id}")))
}

async fn query_for_thing<T>(id:&Thing, col:T) -> Result<Thing> where T:AsRef<str>
{
	let query_context=format!("querying for {} in {id}",col.as_ref());
	let res:Option<Thing> = db()
		.query(format!("select {} from $id",col.as_ref()))
		.bind(("id",id.to_owned())).await
		.and_then(Response::check)
		.and_then(|mut r|r.take(col.as_ref()))
		.map_err(|e|DBErr::from(e).context(&query_context))?;
	res.ok_or(DBErr::NotFound.context(query_context))
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
		_ => {Err(DBErr::InvalidTable {table:id.tb.to_string()}.context(query_context))}
	}
}

#[cfg(feature = "embedded")]
pub async fn init_local(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.to_string_lossy()).as_str());
	let file = format!("file://{file}");
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

	db().query(r#"
	define event add_instance on table instances when $event = "CREATE"	then
	(
		update $after.series set instances += $after.id return none
	)
	"#).await?;
	db().query(r#"
	define event add_series on table series when $event = "CREATE" then
	(
		update $after.study set series += $after.id return none
	)
	"#).await?;

	db().query(r#"
	define event del_instance on table instances when $event = "DELETE" then
	(
		if array::len($before.series.instances)>1
		then
			update $before.series set instances -= $before.id return none
		else
			delete $before.series
		end
	)
	"#).await?;
	db().query(r#"
	define event del_series on table series when $event = "DELETE" then
	(
		if array::len($before.study.series)>1
		then
			update $before.study set series -= $before.id return none
		else
			delete $before.study
		end
	)
	"#).await?;
	Ok(())
}

pub async fn version() -> surrealdb::Result<String>
{
	Ok(format!("{}",db().version().await?))
}
