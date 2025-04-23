use base64::engine::general_purpose;
use base64::Engine;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::vec::IntoIter;
use surrealdb::sql::Id;
use surrealdb::{sql, RecordIdKey, Value};
use surrealdb::opt::{IntoResource, Resource};

#[derive(Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct RecordId(pub surrealdb::RecordId);

impl RecordId {
	fn str_to_vec(s: &str) -> IntoIter<i64>
	{
		let s = format!("{s:+<64}").replace(".","+");
		let bytes = general_purpose::STANDARD.decode(s).unwrap();
		let mut bytes = bytes.as_slice();
		let mut big:Vec<i64>=vec![];
		while !bytes.is_empty() {
			let (head,rest) = bytes.split_at(size_of::<i64>());
			bytes = rest;
			big.push(i64::from_ne_bytes(head.try_into().unwrap()));
		}
		big.into_iter()
	}

	pub fn from_instance(instance_id: &str, series_id: &str, study_id: &str ) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.chain(Self::str_to_vec(instance_id))
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("instances",index)))
	}
	pub fn from_series(series_id: &str, study_id: &str) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("series",index)))
	}
	pub fn from_study(id: &str) -> RecordId
	{
		let index:Vec<_> = Self::str_to_vec(id)
			.map(RecordIdKey::from).map(surrealdb::Value::from)
			.collect();
		RecordId(surrealdb::RecordId::from(("studies",index)))
	}
	pub fn str_key(&self) -> String {
		let bytes= self.key_vec();
		// the last 6 numbers in the vector are the actual ID (not parents)
		let bytes:Vec<_> = bytes.split_at(bytes.len()-6).1.to_vec().into_iter()
			.map(i64::try_from)
			.map(|v|v.unwrap().to_ne_bytes())
			.flatten().collect();
		general_purpose::STANDARD.encode(bytes)
			.trim_end_matches("+").to_string()
			.replace("+",".")
	}
	pub fn key_vec(&self) -> &[sql::Value] {
		if let Id::Array(key) = self.deref().key().into_inner_ref() {
			if key.0.len() == 1 { //aggregate ids are arrays of arrays, just flatten that
				if let sql::Value::Array(array)= &key.0[0]{
					return array.0.as_slice()
				}
			}
			key.0.as_slice()
		}
		else  {panic!("Only vector IDs are allowed")}
	}

	pub fn to_aggregate(&self) -> surrealdb::RecordId {
		let me = Value::from(self.0.clone());
		match self.table() {
			"series" => surrealdb::RecordId::from_table_key("instances_per_series", vec![me]),
			"studies" => surrealdb::RecordId::from_table_key("instances_per_studies",vec![me]),
			_ => panic!("cannot get aggregate data for {self}")
		}
	}
}

impl Eq for RecordId {}
impl Display for RecordId {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_fmt(format_args!("{}:{}",self.table(),self.str_key()))
	}
}

impl Into<Value> for RecordId
{
	fn into(self) -> Value {
		surrealdb::Value::from(self.0)
	}
}

impl Deref for RecordId {
	type Target = surrealdb::RecordId;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl Ord for RecordId {
	fn cmp(&self, other: &Self) -> Ordering {
		let tb=self.0.table().cmp(&other.0.table());
		match tb {
			Ordering::Equal => self.0.key().partial_cmp(&other.0.key()).expect("Failed to compare record keys"),
			_ => tb,
		}
	}
}

impl<S> From<(S,Vec<Value>)> for RecordId where S: Into<String>
{
	fn from(value: (S, Vec<Value>)) -> Self {
		let (table,key) = value;
		RecordId(surrealdb::RecordId::from_table_key(table,key))
	}
}

impl IntoResource<Value> for RecordId {
	fn into_resource(self) -> surrealdb::Result<Resource> {
		Ok(Resource::from(self.0))
	}
}

impl IntoResource<Value> for &RecordId {
	fn into_resource(self) -> surrealdb::Result<Resource> {
		Ok(Resource::from(&self.0))
	}
}
