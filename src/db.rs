use std::collections::HashMap;
use dicom::object::mem::InMemElement;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use serde::{Deserialize, Serialize};
use surrealdb::opt::RecordId;
use crate::dicom_serde::ElementAdapter;

static DB: Surreal<Any> = Surreal::init();

#[derive(Serialize)]
struct MetaData<'a>{
	meta:HashMap<String,Option<ElementAdapter<'a>>>
}

#[derive(Deserialize,Serialize,Debug)]
struct Instance{
	id:RecordId,
	meta: serde_json::Value
}

impl<'a> From<HashMap<String,Option<&'a InMemElement>>> for  MetaData<'a>{
	fn from(meta: HashMap<String, Option<&'a InMemElement>>) -> Self {
		MetaData::<'a>{
			meta:meta.into_iter()
				.map(|(k,v)|	(k,v.map(|e| ElementAdapter::from(e))))
				.collect()
		}
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
	Ok(())
}

pub async fn register(uid:&str,meta:HashMap<String,Option<&InMemElement>>) -> surrealdb::Result<()>
{
	let x:Instance = DB
		.create(("instances",uid))
		.content(MetaData::from(meta)).await?;
	println!("{x:#?}");
	Ok(())
}
