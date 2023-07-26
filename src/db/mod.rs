use std::collections::BTreeMap;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use surrealdb::sql;
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
	Ok(())
}

//,meta:HashMap<String,Option<&InMemElement>>
pub async fn register_instance(uid:&str,meta:BTreeMap<String,sql::Value>) -> surrealdb::Result<()>
{
	let query= r"CREATE $instance CONTENT $data";
	let res = DB.query(query)
		.bind(("instance",sql::Thing{ tb: "instances".into(), id: uid.into() }))
		.bind(("data",meta)).await?;
	Ok(())
}
