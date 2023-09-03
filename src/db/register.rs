use std::collections::BTreeMap;
use std::sync::OnceLock;
use surrealdb::Result;
use surrealdb::opt::IntoQuery;
use surrealdb::sql::Statement;
use crate::{DbVal,JsonVal};

static INSERT_STUDY:OnceLock<Vec<Statement>> = OnceLock::new();
static INSERT_SERIES:OnceLock<Vec<Statement>> = OnceLock::new();
static INSERT_INSTANCE:OnceLock<Vec<Statement>> = OnceLock::new();

pub async fn register(
	instance_meta:BTreeMap<String,DbVal>,
	series_meta:BTreeMap<String,DbVal>,
	study_meta: BTreeMap<String, DbVal>
) -> Result<JsonVal>
{
	let ins_study= INSERT_STUDY.get_or_init(||"INSERT INTO studies $study_meta return before".into_query().unwrap());
	let ins_series = INSERT_SERIES.get_or_init(||"INSERT INTO series $series_meta return before".into_query().unwrap());
	let ins_inst = INSERT_INSTANCE.get_or_init(||"INSERT INTO instances $instance_meta return before".into_query().unwrap());
	let mut res= super::db()
		.query(ins_study.clone())
		.query(ins_series.clone())
		.query(ins_inst.clone())
		.bind(("instance_meta",instance_meta))
		.bind(("series_meta",series_meta))
		.bind(("study_meta",study_meta))
		.await?.check()?;
	let instance = res.take::<Option<JsonVal>>(2)?.unwrap_or(JsonVal::Null);
	Ok(instance)
}
