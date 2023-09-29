
use std::path::Path;
use std::sync::OnceLock;
use anyhow::anyhow;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::{Surreal,Result};
use surrealdb::Error::Api;
use surrealdb::error::Api::Query;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::Thing;
use crate::JsonVal;

#[cfg(feature = "html")]
use crate::server::html_item::HtmlItem;

mod into_db_value;
mod register;
mod entry;

pub(crate) use into_db_value::IntoDbValue;
pub(crate) use register::register;
#[cfg(feature = "html")]
pub(crate) use entry::Entry;

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

pub async fn list<T>(table:T) -> Result<Vec<JsonVal>> where T:AsRef<str> {
	db().select(table.as_ref()).await
}

pub async fn query_for_entry(id:Thing) -> Result<JsonVal>
{
	let res:Option<JsonVal> = db().select(id).await?;
	Ok(res.unwrap_or(JsonVal::Null))
}

pub async fn query_for_thing<T>(id:&Thing, col:T) -> Result<Thing> where T:AsRef<str>
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

pub async fn unregister(id:Thing) -> Result<JsonVal>
{
	let res:Option<JsonVal> = db().delete(id).await?;
	Ok(res.unwrap_or(JsonVal::Null))
}

pub async fn init_local(file:&Path) -> anyhow::Result<()>
{
	let file = format!("file://{}",file.to_str().ok_or(anyhow!(r#""{}" is an invalid filename"#,file.to_string_lossy()))?);
	db().connect(file).await?;
	init().await.map_err(|e|e.into())
}
pub async fn init_remote(addr:&str) -> Result<()> {
	db().connect(addr).await?;

	// Signin as a namespace, database, or root user
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

pub fn json_id_cleanup(val:&JsonVal) -> anyhow::Result<JsonVal>
{
	if let JsonVal::Array(list) = val
	{
		let res:anyhow::Result<Vec<_>>=list.into_iter()
			.map(|v|json_id_cleanup(v))
			.collect();
		res.map(|list|JsonVal::from(list)).map_err(|e|e.into())
	} else if val.is_object() {
		let id= json_to_thing(val.to_owned())?;
		Ok(JsonVal::from(format!("{}:{}",id.tb,id.id)))
	} else {
		Err(anyhow!("Json value {val} should be an array or an object"))
	}
}

pub async fn version() -> Result<String>
{
	Ok(format!("{}",db().version().await?))
}

pub fn json_to_thing(v:JsonVal) -> anyhow::Result<Thing>{
	let (tb, v) = extract_json("tb",v)?;
	let (id,_) = extract_json("String", extract_json("id",v)?.0)?;

	if let JsonVal::String(tb) = tb {
		if let JsonVal::String(id) = id{
			Ok(Thing::from((tb,id)))
		} else { Err(anyhow!("{id} should be a string")) }
	} else {Err(anyhow!("{tb} should be a string"))}
}

fn extract_json(key:&str,mut json_ob:JsonVal) -> anyhow::Result<(JsonVal,JsonVal)>
{
	match json_ob.as_object_mut(){
		None => Err(anyhow!("{json_ob} must be an object")),
		Some(o) => o.remove(key).ok_or(anyhow!("expected {key} in {json_ob}"))
	}.and_then(|extracted|Ok((extracted,json_ob)))
}
