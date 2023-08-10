use std::collections::BTreeMap;
use surrealdb::Result;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::Statement;
use crate::db::{DB, DbVal, JsonValue};
use once_cell::sync::Lazy;

static INSERT_STUDY:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO studies $study_meta return before".into_query().unwrap());
static INSERT_SERIES:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO series $series_meta return before".into_query().unwrap());
static INSERT_INSTANCE:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO instances $instance_meta return before".into_query().unwrap());

pub async fn register(
	instance_meta:BTreeMap<String,DbVal>,
	series_meta:BTreeMap<String,DbVal>,
	study_meta: BTreeMap<String, DbVal>
) -> Result<JsonValue>
{
	let mut res= DB
		.query(INSERT_STUDY.clone())
		.query(INSERT_SERIES.clone())
		.query(INSERT_INSTANCE.clone())
		.bind(("instance_meta",instance_meta))
		.bind(("series_meta",series_meta))
		.bind(("study_meta",study_meta))
		.await?.check()?;
	let instance = res.take::<Option<JsonValue>>(2)?.unwrap_or(JsonValue::Null);
	Ok(instance)
}
