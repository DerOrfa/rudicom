use base64::engine::general_purpose;
use base64::Engine;
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::vec::IntoIter;
use surrealdb::types as db_types;
use surrealdb::types::{SurrealValue, Value};

#[derive(Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct RecordId(pub db_types::RecordId);

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
		let index = Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.chain(Self::str_to_vec(instance_id))
			.map(|v|v.into_value());
		RecordId(db_types::RecordId::new("instances",db_types::Array::from_iter(index)))
	}
	pub fn from_series(series_id: &str, study_id: &str) -> RecordId
	{
		let index= Self::str_to_vec(study_id)
			.chain(Self::str_to_vec(series_id))
			.map(|v|v.into_value());
		RecordId(db_types::RecordId::new("series",db_types::Array::from_iter(index)))
	}
	pub fn from_study(id: &str) -> RecordId
	{
		let index = Self::str_to_vec(id)
			.map(|v|v.into_value());
		RecordId(db_types::RecordId::new("studies",db_types::Array::from_iter(index)))
	}
	pub fn str_key(&self) -> String {
		let bytes= self.key_vec();
		// the last 6 numbers in the vector are the actual ID (not parents)
		let bytes:Vec<_> = bytes.split_at(bytes.len()-6).1.to_vec().into_iter()
			.map(|v|v.as_i64().unwrap().to_owned())
			.map(|v|v.to_ne_bytes())
			.flatten().collect();
		general_purpose::STANDARD.encode(bytes)
			.trim_end_matches("+").to_string()
			.replace("+",".")
	}
	pub fn str_path(&self) -> String {
		format!("/api/{}/{}",self.table,self.str_key())
	}
	pub fn key_vec(&self) -> &[db_types::Value] {
		if let db_types::RecordIdKey::Array(key) = &self.deref().key {
			if key.len() == 1 { //aggregate ids are arrays of arrays, just flatten that
				if let db_types::Value::Array(array)= &key[0]{
					return array.as_slice()
				}
			}
			key.as_slice()
		}
		else  {panic!("Only vector IDs are allowed")}
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