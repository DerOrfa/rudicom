#![allow(dead_code)]
pub mod dcm;

use rudicom::config;
use rudicom::db;
use std::ops::Deref;
use std::path::Path;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

pub async fn init_db() -> Result<&'static Surreal<Any>, Box<dyn std::error::Error>>
{
	init_config()?;
	db::init_local("memory").await?;
	db::DB.use_ns("namespace").use_db("database").await
		.map_err(|e|format!("Selecting database and namespace failed: {e}"))?;
	
	db::DB.query(include_str!("../../src/db/init.surql")).await.map(|_|())?;
	Ok(db::DB.deref())
}

pub fn init_config() -> Result<(), Box<dyn std::error::Error>> {
	let storage_path = Path::new("/tmp/db_store");
	if !storage_path.exists() {
		std::fs::create_dir(&storage_path)?;
	};

	config::init(None)?;
	Ok(())
}
