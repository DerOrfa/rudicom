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
use surrealdb::opt::{IntoQuery, Resource};
use surrealdb::sql::Id;
use surrealdb::{sql, RecordIdKey, Surreal};
use surrealdb::{Object, Value};

mod into_db_value;
mod register;
mod entry;
mod file;
mod record;

#[derive(Deserialize,Debug)]
pub struct AggregateData
{
	id:surrealdb::RecordId,
	pub count:usize,
	pub size:u64, 
}

impl AggregateData
{
	pub fn get_inner_id(&self) -> RecordId
	{
		let ret = if let Id::Array(array) = self.id.key().into_inner_ref()
		{
			let inner_id_value= array.get(0)
				.expect("aggregate RecordIdKeys must be arrays with one element");
			if let sql::Value::Thing(id) = inner_id_value {
				let inner_key = RecordIdKey::try_from(Value::from_inner(id.clone().id.into()))
					.expect("aggregate RecordIdKeys must be arrays of RecordIds");
				surrealdb::RecordId::from_table_key(id.tb.clone(),inner_key)
			} else {panic!("aggregate RecordIdKeys must be arrays of RecordIds")}
		} else {panic!("aggregate RecordIdKeys must be arrays")};
		RecordId(ret)
	}
}

pub(crate) static DB: LazyLock<Surreal<Any>> = LazyLock::new(Surreal::init);

async fn query(qry:impl IntoQuery, bindings: impl Serialize+'static) -> surrealdb::Result<Value>
{
	let mut result= DB
		.query(qry)
		.bind(bindings)
		.await?;
	result.take::<Value>(0usize)
}

pub(crate) async fn list_entries<T>(table:T) -> Result<Vec<Entry>> where Resource: From<T>
{
	let val = DB.select::<Value>(Resource::from(table)).await?.into_inner();
	let kind = val.kindof();
	if let sql::Value::Array(array) = val {
		array.0.into_iter().map(Value::from_inner).map(Entry::try_from).collect()
	} else {
		Err(UnexpectedResult {found:kind,expected:"list of entries".into()})
	}
}

pub(crate) async fn lookup(id:RecordId) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {id}");
	let v:Value = DB.select(id).await.context(ctx.clone())?;
	if v.into_inner_ref().is_some(){
		Some(Entry::try_from(v)).transpose().context(ctx)
	} else {
		Ok(None)
	}
}

pub(crate) async fn lookup_uid<S:AsRef<str>>(table:S, uid:String) -> Result<Option<Entry>>
{
	let ctx = format!("looking up {uid} in {}",table.as_ref());
	let value= query(format!("select * from {} where uid == $uid",table.as_ref()), ("uid", uid))
		.await.context(ctx.clone())?;
	if value.into_inner_ref().is_truthy() {
		Some(Entry::try_from(value)).transpose().context(ctx)
	}else{
		Ok(None)
	}

}

/// returns [me,parent,parents_parent]
pub fn find_down_tree(id:RecordId) -> Result<Vec<RecordId>>
{
	let query_context = format!("looking for parents of {id}");
	let key_vec:Vec<_> = id.key_vec().to_vec().into_iter().map(Value::from_inner).collect();
	match id.table() {
		"instances" => {
			let (study,_) = key_vec.split_at(6); // just study
			let (series, _) = key_vec.split_at(12); // study + series
			Ok(vec![
				id,
				("series",series.to_vec()).into(),
				("studies",study.to_vec()).into()
			])
		},
		"series" => {
			let (study,_) = key_vec.split_at(6);
			Ok(vec![
				id,("studies",study.to_vec()).into()
			])
		},
		"studies" => Ok(vec![id]),
		_ => {Err(Error::InvalidTable {table:id.table().to_string()}.context(query_context))}
	}
}

pub async fn init_file(file:&std::path::Path) -> surrealdb::Result<()>
{
	let file = file.to_str().expect(format!(r#""{}" is an invalid filename"#,file.to_string_lossy()).as_str());
	init_local(format!("surrealkv://{file}").as_str()).await
}
pub async fn init_remote(addr:&str) -> surrealdb::Result<()>
{
	DB.connect(addr).await?;

	// Sign in as a namespace, database, or root user
	DB.signin(Root { username: "root", password: "root", }).await?;
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
		let slf = self.into_inner_ref();
		let kind = slf.kindof();
		match slf {
			sql::Value::Object(obj) => Object::from_inner_ref(obj).pick_ref(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let slf = self.into_inner_mut();
		let kind = slf.kindof();
		match slf {
			sql::Value::Object(obj) => Object::from_inner_mut(obj).pick_remove(element),
			_ => Err(UnexpectedResult {expected:"entry object".into(),found:kind})
		}
	}
}

impl Pickable for Object {
	fn pick_ref<Q>(&self, element:Q) -> Result<&Value> where String: From<Q>{
		let element = String::from(element);
		let slf = self.into_inner_ref();
		slf.get(&element).map(Value::from_inner_ref)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}

	fn pick_remove<Q>(&mut self, element:Q) -> Result<Value> where String: From<Q> {
		let element = String::from(element);
		self.into_inner_mut().remove(&element).map(Value::from_inner)
			.ok_or(ElementMissing {element,parent:"object".into()})
	}
}
