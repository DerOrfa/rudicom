use std::collections::BTreeMap;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use surrealdb::sql::Value as DbVal;
pub(crate) use serde_json::Value as JsonValue;

mod into_db_value;
mod register;

pub(crate) use into_db_value::IntoDbValue;

static DB: Surreal<Any> = Surreal::init();

pub struct Entry{
	id:surrealdb::sql::Thing,
	data:JsonValue,
	created:bool,
}

impl Into<JsonValue> for Entry{
	fn into(self) -> JsonValue {
		self.data
	}
}

impl Entry{
	pub fn is_created(&self) -> bool{
		self.created
	}
}

pub async fn register(
	instance_meta:BTreeMap<String,DbVal>,
	series_meta:BTreeMap<String,DbVal>,
	study_meta: BTreeMap<String, DbVal>
) -> surrealdb::Result<Entry>{
	register::register(&DB, instance_meta, series_meta, study_meta).await
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
	DB.query(r"DEFINE INDEX unique_relationships ON TABLE contains COLUMNS in, out UNIQUE").await?;
	Ok(())
}

