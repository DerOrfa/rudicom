
use std::path::Path;
use std::sync::OnceLock;
use anyhow::{anyhow, Context};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::{Surreal, Result};
use surrealdb::Error::Api;
use surrealdb::error::Api::Query;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::{Object,Value, Thing};

mod into_db_value;
mod register;
mod entry;
mod file;

pub use into_db_value::IntoDbValue;
pub use register::{unregister,register,register_instance,RegistryGuard};
pub use entry::Entry;
pub use file::File;

static DB: OnceLock<Surreal<Any>> = OnceLock::new();

fn db() -> &'static Surreal<Any>
{
	DB.get_or_init(Surreal::init)
}

pub async fn query_for_list(id:Thing,target:&str) -> Result<Vec<Thing>>
{
	let qry=format!("select array::flatten({target}) as id from $id").into_query()?;
	let res:Option<Vec<Thing>>=db()
		.query(qry.clone())
		.bind(("id",id))
		.await?.check()?
		.take("id")?;
	Ok(res.unwrap_or(Vec::new()))
}

pub(crate) async fn list_table<T>(table:T) -> anyhow::Result<Vec<Entry>> where T:AsRef<str>
{
	db().select::<Vec<Object>>(table.as_ref()).await?
		.into_iter().map(Entry::try_from).collect()
}

pub(crate) async fn list_values<T>(id:&Thing, col:T) -> anyhow::Result<Vec<Value>> where T:AsRef<str>
{
	if let Some(values) = db().query(format!("select * from $id.{}",col.as_ref()))
		.bind(("id",id)).await?.check()?
		.take::<Option<Value>>(0)?.map(|v|v.flatten())
	{
		Ok(match values {
			Value::Array(v) => v.0,
			_ => vec![values]
		})
	}
	else
	{
		Ok(vec![])
	}
}
pub(crate) async fn list_children<T>(id:&Thing, col:T) -> anyhow::Result<Vec<Entry>> where T:AsRef<str>
{
	list_values(id, col).await?.into_iter()
		.map(|v|
			if let Value::Object(o)=v{Entry::try_from(o)}
			else {Err(anyhow!(r#""{v}" is not an object"#))}
		).collect()
}
pub(crate) async fn lookup(id:&Thing) -> anyhow::Result<Option<Entry>>
{
	db().select::<Option<Object>>(id).await.context(format!("failed looking up {}", id))?
		.map(Entry::try_from).transpose()
		.map_err(|e|e.context(format!("when looking up {}", id)))
}

async fn query_for_thing<T>(id:&Thing, col:T) -> Result<Thing> where T:AsRef<str>
{
	let res:Option<Thing> = db()
		.query(format!("select {} from $id",col.as_ref()))
		.bind(("id",id.to_owned())).await?.check()?
		.take(col.as_ref())?;
	res.ok_or(Api(Query(format!("{} not in {id}",col.as_ref()))))
}

pub async fn find_down_tree(id:&Thing) -> Result<Vec<Thing>>
{
	match id.tb.as_str() {
		"instances" => {
			let series = query_for_thing(id, "series").await?;
			let study = query_for_thing(&series, "study").await?;
			Ok(vec![id.to_owned(),series,study])
		},
		"series" => {
			let study = query_for_thing(id, "study").await?;
			Ok(vec![id.to_owned(), study])
		},
		"studies" => Ok(vec![id.to_owned()]),
		_ => {Err(Api(Query("invalid db table name when looking for parents".to_string())))}
	}
}

pub async fn init_local(file:&Path) -> anyhow::Result<()>
{
	let file = format!("file://{}",file.to_str().ok_or(anyhow!(r#""{}" is an invalid filename"#,file.to_string_lossy()))?);
	db().connect(file).await?;
	init().await.map_err(|e|e.into())
}
pub async fn init_remote(addr:&str) -> Result<()>
{
	db().connect(addr).await?;

	// Sign in as a namespace, database, or root user
	db().signin(Root { username: "root", password: "root", }).await?;
	init().await
}
async fn init() -> Result<()>{
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

pub async fn version() -> Result<String>
{
	Ok(format!("{}",db().version().await?))
}
