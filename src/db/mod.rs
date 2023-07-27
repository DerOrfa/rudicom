use std::collections::BTreeMap;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use surrealdb::sql::Value;

mod into_db_value;

pub(crate) use into_db_value::IntoDbValue;

static DB: Surreal<Any> = Surreal::init();

pub async fn init(addr:&str) -> surrealdb::Result<()>{
	DB.connect(addr).await?;

	// Signin as a namespace, database, or root user
	DB.signin(Root {
		username: "root",
		password: "root",
	}).await?;

	// Select a specific namespace / database
	DB.use_ns("namespace").use_db("database").await?;
	DB.query(r"DEFINE INDEX unique_relationships ON TABLE contains COLUMNS in, out UNIQUE").await?;
	Ok(())
}

//,meta:HashMap<String,Option<&InMemElement>>
pub async fn register_instance(
	mut instance_meta:BTreeMap<String,Value>,
	mut series_meta:BTreeMap<String,Value>,
	mut study_meta: BTreeMap<String, Value>
) -> surrealdb::Result<()>
{
	let instance_uid = instance_meta.get("id").cloned().expect("\"id\" is missing in instance_meta");
	let series_uid = series_meta.get("id").cloned().expect("\"id\" is missing in series_meta");
	let study_uid = study_meta.get("id").cloned().expect("\"id\" is missing in series_meta");

	let res = DB
		.query(r"INSERT INTO instances $instance_data")
		.query(r"INSERT INTO series $series_data")
		.query(r"RELATE $series->contains->$instance")
		.query(r"INSERT INTO studies $study_data")
		.query(r"RELATE $study->contains->$series")
		.bind(("instance_data",instance_meta))
		.bind(("series_data",series_meta))
		.bind(("study_data",study_meta))
		.bind(("instance",instance_uid))
		.bind(("series",series_uid))
		.bind(("study",study_uid))
		.await?;
	// println!("{res:#?}");
	Ok(())
}
