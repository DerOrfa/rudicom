use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use surrealdb::types as db_types;
use surrealdb::types::{RecordIdKey, SurrealValue, Value};

#[derive(Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct RecordId(pub db_types::RecordId);

impl RecordId {
	pub fn from_instance(instance_id: impl Into<RecordIdKey>) -> RecordId
	{
		RecordId(db_types::RecordId::new("instances",instance_id))
	}
	pub fn from_series(series_id: impl Into<RecordIdKey>) -> RecordId
	{
		RecordId(db_types::RecordId::new("series",series_id))
	}
	pub fn from_study(id: impl Into<RecordIdKey>) -> RecordId
	{
		RecordId(db_types::RecordId::new("studies",id))
	}
	pub fn str_key(&self) -> String {
		self.0.key.clone().into_value().into_string().unwrap()
	}
	pub fn str_path(&self) -> String {
		format!("/api/{}/{}",self.table,self.str_key())
	}
	pub fn to_aggregate(&self) -> db_types::RecordId {
		let me = db_types::Array::from(vec![self.0.clone()]);
		match self.table.as_str() {
			"series" => db_types::RecordId::new("instances_per_series", me),
			"studies" => db_types::RecordId::new("instances_per_studies",me),
			_ => panic!("cannot get aggregate data for {self}")
		}
	}
}

impl Eq for RecordId {}
impl Display for RecordId {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_fmt(format_args!("{}:{}",self.table,self.str_key()))
	}
}

impl Deref for RecordId {
	type Target = db_types::RecordId;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
impl Ord for RecordId {
	fn cmp(&self, other: &Self) -> Ordering {
		let tb=self.0.table.cmp(&other.0.table);
		match tb {
			Ordering::Equal => self.key.partial_cmp(&other.key).expect("Failed to compare record keys"),
			_ => tb,
		}
	}
}

impl From<(&str,&[db_types::Value])> for RecordId {
	fn from(value: (&str, &[Value])) -> Self {
		let array = db_types::Array::from_iter(value.1.iter().cloned());
		RecordId(db_types::RecordId::new(value.0, array))
	}
}