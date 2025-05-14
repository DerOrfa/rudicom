use std::collections::BTreeMap;
use surrealdb::sql;
use crate::db::RecordId;
use serde_json::value::Value as JValue;

pub fn value_to_json(value: sql::Value) -> serde_json::value::Value
{
	match value {
		sql::Value::None | sql::Value::Null => serde_json::Value::Null,
		sql::Value::Bool(b) => serde_json::Value::Bool(b),
		sql::Value::Number(num) => {
			match num {
				sql::Number::Int(i) => serde_json::Value::Number(i.into()),
				sql::Number::Float(f) => serde_json::Value::from(f),
				_ => serde_json::Value::String(num.to_string()),
			}
		}
		sql::Value::Strand(s) => serde_json::Value::String(s.0),
		sql::Value::Array(a) => a.into_iter().map(value_to_json).collect(),
		sql::Value::Object(o) => serde_json::Value::Object(
			o.into_iter().map(|(k,v)| (k, value_to_json(v))).collect()
		),
		sql::Value::Thing(id) => RecordId(surrealdb::RecordId::from_inner(id.into())).str_key().into(),
		_ => serde_json::Value::String(value.to_string()),
	}
}

pub fn json_to_value(json_val: JValue) -> sql::Value {
	match json_val{
		JValue::Null => sql::Value::None,
		JValue::Bool(b) => b.into(),
		JValue::String(s) => s.into(),
		JValue::Number(n) => {
			if n.is_f64() {
				n.as_f64().expect("f64 should be representable as f64").into()
			} else if n.is_i64() {
				n.as_i64().expect("i64 should be representable as i64").into()
			} else if n.is_u64() {
				n.as_u64().expect("u64 should be representable as i64").into()
			} else {unreachable!()}	
		}			
		JValue::Array(a) =>
			a.into_iter().map(json_to_value).collect::<Vec<sql::Value>>().into(),
		JValue::Object(o) =>
			o.into_iter().map(|(k,v)| (k,json_to_value(v)))
				.collect::<BTreeMap<String, sql::Value>>().into()
	}
}
