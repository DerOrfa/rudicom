use std::collections::BTreeMap;
use dicom::object::{DefaultDicomObject, Tag};
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
pub(crate) use surrealdb::sql::Value as DbVal;
pub(crate) use serde_json::Value as JsonValue;
use surrealdb::sql::Thing;

mod into_db_value;
mod register;

pub(crate) use into_db_value::IntoDbValue;
use crate::dcm::{extract};

static DB: Surreal<Any> = Surreal::init();

pub fn prepare_meta_for_db(obj:&DefaultDicomObject, attrs: Vec<(&str, Tag)>, table:&str, id_tag:Tag) -> anyhow::Result<BTreeMap<String,DbVal>>{
	let id = obj.element(id_tag)?.to_str()?;
	let extracted = extract(obj,attrs);
	let meta:BTreeMap<_,_> = extracted.into_iter()
			.map(|(k,v)|{
				(String::from(k),v.cloned().into_db_value())
			})
			.chain([
				(String::from("id"),DbVal::Thing(Thing::from((table,id.as_ref()))))
			])
			.collect();
	Ok(meta)
}

pub async fn register(
	instance_meta:BTreeMap<String,DbVal>,
	series_meta:BTreeMap<String,DbVal>,
	study_meta:BTreeMap<String,DbVal>
) -> surrealdb::Result<JsonValue>{
	register::register(&DB, instance_meta, series_meta, study_meta).await
}

pub async fn init(addr:&str) -> surrealdb::Result<()>{
	DB.connect(addr).await?;

	// Signin as a namespace, database, or root user
	DB.signin(Root { username: "root", password: "root", }).await?;

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

