use std::collections::BTreeMap;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
pub(crate) use surrealdb::sql::Value as DbVal;
pub(crate) use serde_json::Value as JsonValue;

mod into_db_value;
mod register;

pub(crate) use into_db_value::IntoDbValue;

static DB: Surreal<Any> = Surreal::init();

pub async fn register(
	instance_meta:BTreeMap<String,DbVal>,
	series_meta:BTreeMap<String,DbVal>,
	study_meta: BTreeMap<String, DbVal>
) -> surrealdb::Result<JsonValue>{
	match register::register(&DB, instance_meta, series_meta, study_meta).await {
		Ok(entry) => Ok(entry),
		Err(e) => {println!("{e}");Err(e)}
	}

}

pub async fn init(addr:&str) -> surrealdb::Result<()>{
	DB.connect(addr).await?;

	// Signin as a namespace, database, or root user
	DB.signin(Root {
		username: "root",
		password: "root",
	}).await?;

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

