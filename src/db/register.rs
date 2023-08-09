use std::collections::BTreeMap;
use surrealdb::{Connection, Surreal, Result};
use surrealdb::opt::IntoQuery;
use surrealdb::sql::{Statement,Value::Thing};
use crate::db::{DbVal, JsonValue};
use once_cell::sync::Lazy;

static INSERT_STUDY:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO studies $study_meta return before".into_query().unwrap());
static INSERT_SERIES:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO series $series_meta return before".into_query().unwrap());
static INSERT_INSTANCE:Lazy<Vec<Statement>> =
	Lazy::new(||"INSERT INTO instances $instance_meta return before".into_query().unwrap());

pub async fn register<C>(db:&Surreal<C>,
						 mut instance_meta:BTreeMap<String,DbVal>,
						 mut series_meta:BTreeMap<String,DbVal>,
						 study_meta: BTreeMap<String, DbVal>
) -> Result<JsonValue>
	where C: Connection
{
	let Thing(study_id) = study_meta.get("id").expect("Study data is missing \"id\"").clone()
		else {panic!("\"id\" in study data is not an id")};
	let Thing(series_id) = series_meta.get("id").expect("Series data is missing \"id\"").clone()
		else {panic!("\"id\" in series data is not an id")};

	instance_meta.insert("series".into(),Thing(series_id));
	series_meta.insert("study".into(),Thing(study_id));

	let mut res= db
		.query(INSERT_STUDY.clone())
		.query(INSERT_SERIES.clone())
		.query(INSERT_INSTANCE.clone())
		.bind(("instance_meta",instance_meta))
		.bind(("series_meta",series_meta))
		.bind(("study_meta",study_meta))
		.await?.check()?;
	let instance = res.take::<Vec<JsonValue>>(2)?.remove(0);
	Ok(instance)
}
