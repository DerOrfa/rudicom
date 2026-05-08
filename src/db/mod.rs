use std::borrow::Cow;
use std::collections::BTreeMap;
use crate::tools::Error::{ElementMissing, UnexpectedResult};
use crate::tools::{Context, Error, Result};
use byte_unit::Byte;
use byte_unit::UnitType::Binary;
pub use entry::Entry;
pub use file::File;
pub use into_db_value::IntoDbValue;
pub use register::{register_instance, RegistryGuard};
pub use record::RecordId;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::opt::{IntoResource, PatchOp, Resource};
use surrealdb::Surreal;
use surrealdb::method::IntoVariables;
use surrealdb::types::{RecordIdKey, SurrealValue, Value};
use surrealdb::types as db_types;


mod into_db_value;
mod register;
mod entry;
mod file;
mod record;

#[derive(Deserialize,Debug,SurrealValue)]
pub struct AggregateData
{
	id:surrealdb::types::RecordId,
	pub count:usize,
	pub size:u64, 
}

impl AggregateData
{
	pub fn get_inner_id(&self) -> RecordId
	{
		let ret = if let RecordIdKey::Array(array) = &self.id.key
		{
			let inner_id_value= array.get(0)
				.expect("aggregate RecordIdKeys must be arrays with one element");
			if let Value::RecordId(id) = inner_id_value {
				let inner_key = &id.key;
				surrealdb::types::RecordId{table:id.table.to_owned(),key:inner_key.to_owned()}
			} else {panic!("aggregate RecordIdKeys must be arrays of RecordIds")}
		} else {panic!("aggregate RecordIdKeys must be arrays")};
		RecordId(ret)
	}
}

pub static DB: LazyLock<Surreal<Any>> = LazyLock::new(Surreal::init);

#[derive(Debug)]
pub enum RegisterResult { // @todo unify via implementing Try https://github.com/rust-lang/rust/issues/84277
	Stored(RecordId),
	AlreadyStored(RecordId),
}

async fn query(qry: impl Into<Cow<'_, str>>, bindings: impl IntoVariables) -> surrealdb::Result<Value>
{
	let mut result= DB
		.query(qry)
		.bind(bindings)
		.await?;
	result.take::<Value>(0usize)
}

pub async fn list_entries<T>(table:T) -> Result<Vec<Entry>> where Resource: From<T>
{
	let val = DB.select::<Value>(Resource::from(table)).await?;
	let kind = val.kind().to_string();
	if let Value::Array(array) = val {
		array.into_iter().map(Entry::try_from)
			.collect()
	} else {
		Err(UnexpectedResult {found: kind,expected:"list of entries".into()})
	}
}

pub async fn lookup(id:&RecordId) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {id}");
	let v:Option<Value> = DB.select(id.0.to_owned()).await.context(ctx.clone())?;
	if let Some(v) = v {
		Some(Entry::try_from(v)).transpose().context(ctx)
	} else {
		Ok(None)
	}
}

pub async fn lookup_uid<S:AsRef<str>>(table:S, uid:String) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {uid} in {}",table.as_ref());
	let value= query(format!("select * from {} where uid == $uid",table.as_ref()), ("uid", uid))
		.await.context(ctx.clone())?;
	if value.is_nullish() {
		Ok(None)
	}else{
		Some(Entry::try_from(value)).transpose().context(ctx)
	}

}

/// returns [me,parent,parents_parent]
pub fn find_down_tree(id:RecordId) -> Result<Vec<RecordId>>
{
	let query_context = format!("looking for parents of {id}");
	todo!()
	// let key_vec:Vec<_> = id.key_vec().to_vec().into_iter().collect();
	// match id.table.as_str() {
	// 	"instances" => {
	// 		let (study,_) = key_vec.split_at(6); // just study
	// 		let (series, _) = key_vec.split_at(12); // study + series
	// 		Ok(vec![
	// 			id,
	// 			("series", series).into(),
	// 			("studies",study).into()
	// 		])
	// 	},
	// 	"series" => {
	// 		let (study,_) = key_vec.split_at(6);
	// 		Ok(vec![
	// 			id,("studies",study).into()
	// 		])
	// 	},
	// 	"studies" => Ok(vec![id]),
	// 	_ => {Err(Error::InvalidTable {table:id.table.to_string()}.context(query_context))}
	// }
}

pub async fn init_file(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.display()).as_str());
	init_local(format!("surrealkv://{file}").as_str()).await
}
pub async fn init_remote(addr:&str) -> surrealdb::Result<()>
{
	DB.connect(addr).await?;

	// Sign in as a namespace, database, or root user
	DB.signin(Root { username: "root".into(), password: "root".into(), }).await?;
	Ok(())
}
pub async fn init_local(addr:&str) -> surrealdb::Result<()>
{
	DB.connect(addr).await
}

#[derive(Serialize)]
pub struct Stats
{
	studies:usize,
	instances:usize,
	stored_size:String,
	db_version:String,
	health:String,
	version:String,
}
pub async fn statistics() -> Result<Stats>
{
	let studies_v:Vec<AggregateData> = DB.select("instances_per_studies").await?;

	let size = studies_v.iter()
		.map(|v|Byte::from(v.size))
		.reduce(|a,b|a.add(b).unwrap_or(Byte::MAX)).unwrap_or(Byte::MIN);
	let instances = studies_v.iter()
		.map(|v|v.count)
		.reduce(|a,b|a+b).unwrap_or(0);
	let studies = studies_v.len();
	
	// let instances = instances_v.len();
	let health= match DB.health().await{
		Ok(_) => String::from("good"),
		Err(e) => e.to_string()
	};
	
	Ok(Stats{
		studies,instances,health,version:env!("CARGO_PKG_VERSION").to_string(),
		stored_size:format!("{:.2}",size.get_appropriate_unit(Binary)),
		db_version:DB.version().await?.to_string(),
	})
}

trait Pickable
{
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>;
	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q>;
}

pub enum Table{Studies,Series,Instances}

impl Pickable for Value {
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>{
		let kind = self.kind().to_string();
		match self {
			db_types::Value::Object(obj) => obj.pick_ref(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let kind = self.kind().to_string();
		match self {
			db_types::Value::Object(obj) => obj.pick_remove(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}
}

impl Pickable for db_types::Object {
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>{
		let element = String::from(element);
		self.get(&element)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let element = String::from(element);
		self.remove(&element)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}
}

pub async fn set_value(id:impl IntoResource<Option<Value>>, name:String, value:Value) -> Result<Value> {
	let ctx = format!("Updating column {name}");
	let ob = db_types::Object::from(BTreeMap::<String, db_types::Value>::from([(name,value)]));
	DB.update(id).merge(ob).await.map(Option::unwrap_or_default).context(ctx)
}
pub async fn delete_value(id:impl IntoResource<Option<Value>>, name:impl AsRef<str>) -> Result<Value> {
	let ctx = format!("Deleting column {}",name.as_ref());
	
	DB.update(id).patch(PatchOp::remove(name.as_ref())).await.map(Option::unwrap_or_default).context(ctx)
}
