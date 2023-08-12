use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::{Surreal,Result};
use surrealdb::opt::IntoQuery;
use surrealdb::sql::Thing;
use crate::JsonVal;

mod into_db_value;
mod register;

pub(crate) use into_db_value::IntoDbValue;
pub(crate) use register::register;

static DB: Surreal<Any> = Surreal::init();

pub async fn query_for_list(id:Thing,target:&str) -> Result<Vec<Thing>>
{
	let qry=format!("select array::flatten({target}) as id from $id").into_query()?;
	let res:Option<Vec<Thing>>=DB
		.query(qry.clone())
		.bind(("id",id))
		.await?.check()?
		.take("id")?;
	Ok(res.unwrap_or(Vec::new()))
}

pub async fn query_for_entry(id:Thing) -> Result<JsonVal>
{
	let res:Option<JsonVal> = DB.select(id).await?;
	Ok(res.unwrap_or(JsonVal::Null))
}

pub async fn unregister(id:Thing) -> Result<JsonVal>
{
	let res:Option<JsonVal> = DB.delete(id).await?;
	Ok(res.unwrap_or(JsonVal::Null))
}

pub async fn init(addr:&str) -> Result<()>{
	DB.connect(addr).await?;

	// Signin as a namespace, database, or root user
	DB.signin(Root { username: "root", password: "root", }).await?;

	// Select a specific namespace / database
	DB.use_ns("namespace").use_db("database").await?;

	DB.query(r#"
	define event add_instance on table instances when $event = "CREATE"	then
	(
		update type::thing($after.series) set instances += $after.id return none
	)
	"#).await?;
	DB.query(r#"
	define event add_series on table series when $event = "CREATE" then
	(
		update type::thing($after.study) set series += $after.id return none
	)
	"#).await?;

	DB.query(r#"
	define event del_instance on table instances when $event = "DELETE" then
	(
		if array::len($before.series.instances)>1
		then
			update type::thing($before.series) set instances -= $before.id return none
		else
			delete type::thing($before.series)
		end
	)
	"#).await?;
	DB.query(r#"
	define event del_series on table series when $event = "DELETE" then
	(
		if array::len($before.study.series)>1
		then
			update type::thing($before.study) set series -= $before.id return none
		else
			delete type::thing($before.study)
		end
	)
	"#).await?;
	Ok(())
}

