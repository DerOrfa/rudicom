use std::collections::BTreeMap;
use surrealdb::types as db_types;
use crate::db::RecordId;
use serde_json::value::Value as JValue;
use surrealdb::types::{SurrealValue, ToSql};

pub fn value_to_json(value: db_types::Value) -> serde_json::value::Value
{
	match value {
		db_types::Value::None | db_types::Value::Null => serde_json::Value::Null,
		db_types::Value::Bool(b) => serde_json::Value::Bool(b),
		db_types::Value::Number(num) => {
			match num {
				db_types::Number::Int(i) => serde_json::Value::Number(i.into()),
				db_types::Number::Float(f) => serde_json::Value::from(f),
				_ => serde_json::Value::String(num.to_string()),
			}
		}
		db_types::Value::String(s) => serde_json::Value::String(s),
		db_types::Value::Array(a) => a.into_iter().map(value_to_json).collect(),
		db_types::Value::Object(o) => serde_json::Value::Object(
			o.into_iter().map(|(k,v)| (k, value_to_json(v))).collect()
		),
		db_types::Value::RecordId(id) => RecordId(id).str_key().into(),
		_ => serde_json::Value::String(value.to_sql_pretty()), // @todo most likely wrong
	}
}

pub fn json_to_value(json_val: JValue) -> db_types::Value {
	match json_val{
		JValue::Null => db_types::Value::None,
		JValue::Bool(b) => b.into_value(),
		JValue::String(s) => s.into_value(),
		JValue::Number(n) => {
			if n.is_f64() {
				n.as_f64().expect("f64 should be representable as f64").into_value()
			} else if n.is_i64() {
				n.as_i64().expect("i64 should be representable as i64").into_value()
			} else if n.is_u64() {
				n.as_u64().expect("u64 should be representable as i64").into_value()
			} else {unreachable!()}	
		}			
		JValue::Array(a) =>
			a.into_iter().map(json_to_value).collect::<Vec<db_types::Value>>().into_value(),
		JValue::Object(o) =>
			o.into_iter().map(|(k,v)| (k,json_to_value(v)))
				.collect::<BTreeMap<String, db_types::Value>>().into_value(),
	}
}
