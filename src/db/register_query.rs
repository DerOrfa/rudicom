use std::collections::BTreeMap;
use std::sync::OnceLock;
use surrealdb::sql::Value as DbVal;
use surrealdb::sql::Statement;
use surrealdb::opt::IntoQuery;
use surrealdb::Surreal;
use surrealdb::Connection;

static CL: OnceLock<Vec<Statement>> = OnceLock::new();
static QUERY:&str = r"
if select id from $instance then
	select * from $instance
else
[
	create $instance CONTENT $instance_data return none,
	if select id from $series then
	[]
	else
	[
		create $series CONTENT $series_data return none,
		if select id from $study then
		[]
		else
		[
			create $study CONTENT $study_data return none,
		]
		end,
		RELATE $study->contains->$series return none,
	]
	end,
	relate $series->contains->$instance return none,
]
end";

fn extract_query_result(src:Option<serde_json::Value>, depth:usize) -> serde_json::Value{
	match src {
		Some(v) => {
			if depth>0 {extract_query_result(v.as_array().expect("Expected an array").first().cloned(),depth-1)}
			else {
				match v {
					serde_json::Value::Array(ref a) => {if a.is_empty() {serde_json::Value::Null} else {v}},
					_ => {v}
				}
			}
		},
		None => serde_json::Value::Null
	}
}

pub async fn register_query<C>(db:&Surreal<C>,
	mut instance_meta:BTreeMap<String,DbVal>,
	mut series_meta:BTreeMap<String,DbVal>,
	mut study_meta: BTreeMap<String, DbVal>
) -> surrealdb::Result<serde_json::Value>
where C: Connection,
{
	let query = CL.get_or_init(||QUERY.into_query().expect("Failed to parse query")).clone();

	let instance_uid = instance_meta.remove("id").expect("\"id\" is missing in instance_meta");
	let series_uid = series_meta.remove("id").expect("\"id\" is missing in series_meta");
	let study_uid = study_meta.remove("id").expect("\"id\" is missing in series_meta");

	let mut res = db
		.query("BEGIN")
		.query(query)
		.query("COMMIT")
		.bind(("instance_data",instance_meta))
		.bind(("series_data",series_meta))
		.bind(("study_data",study_meta))
		.bind(("instance",instance_uid))
		.bind(("series",series_uid))
		.bind(("study",study_uid))
		.await?;
	let mut result:Vec<serde_json::Value> = res.check()?.take(0)?;
	Ok(extract_query_result(result.pop(),0))
}
