use std::collections::BTreeMap;
use chrono::{Local, TimeZone};
use dicom::core::{DataDictionary, PrimitiveValue};
use dicom::core::chrono;
use dicom::core::value::PreciseDateTime;
use dicom::object::mem::InMemElement;
use dicom::core::value::Value::{Primitive,Sequence};
use dicom::object::{InMemDicomObject, StandardDataDictionary};
use itertools::Itertools;
use surrealdb::sql;

fn flatten_iter<T,I>(iter:I) -> sql::Value
	where I:Iterator<Item=T>, T:Into<sql::Value>
{
	let mut vec:Vec<sql::Value> = iter.map_into().collect();
	match vec.len() {
		0 => sql::Value::None,
		1 => vec.pop().unwrap(),
		_ => vec.into()
	}
}

pub trait IntoDbValue{
	fn into_db_value(self) -> sql::Value;
}

impl IntoDbValue for InMemDicomObject{
	fn into_db_value(self) -> sql::Value {
		let mut obj = BTreeMap::new();
		for e in self{
			let tag = e.header().tag;
			let name = match StandardDataDictionary::default().by_tag(tag) {
				None => tag.to_string(),
				Some(found) => found.alias.to_string()
			};
			let val = e.into_db_value();
			obj.insert(name,val);
		}
		obj.into()
	}
}

impl IntoDbValue for PrimitiveValue {
	fn into_db_value(self) -> sql::Value {
		use PrimitiveValue::*;
		match self {
			Empty => sql::Value::None, // no-op
			Date(dates) => flatten_iter(dates.into_iter().
				map(|date|
					date.to_naive_date().expect("Invalid DICOM timestamp")
						.and_time(chrono::NaiveTime::default())
						.and_utc()
				)
			),
			Time(times) => flatten_iter(times.into_iter()
				.map(|time|
					chrono::NaiveDate::default()
						.and_time(time.to_naive_time().expect("Invalid DICOM timestamp"))
						.and_utc()
				)
			),
			DateTime(datetimes) => flatten_iter(datetimes.into_iter()
				.map(|datetime|
					match datetime.to_precise_datetime().expect("Invalid DICOM timestamp") {
						PreciseDateTime::Naive(dt) => 
							Local.from_local_datetime(&dt).unwrap().to_utc(),
						PreciseDateTime::TimeZone(dt) => dt.to_utc()
					}
				)
			),
			Str(s) => s.trim().into(),
			Strs(s) => flatten_iter(s.into_iter().map(|s|String::from(s.trim()))),
			F32(values) => flatten_iter(values.into_iter()),
			F64(values) => flatten_iter(values.into_iter()),
			U64(values) => flatten_iter(values.into_iter()),
			I64(values) => flatten_iter(values.into_iter()),
			U32(values) => flatten_iter(values.into_iter()),
			I32(values) => flatten_iter(values.into_iter()),
			U16(values) => flatten_iter(values.into_iter()),
			I16(values) => flatten_iter(values.into_iter()),
			U8(values) => flatten_iter(values.into_iter()),
			Tags(tags) => flatten_iter(tags.into_iter().map(|t|t.to_string()))
		}

	}
}

impl IntoDbValue for InMemElement{
	fn into_db_value(self) -> sql::Value {
		match self.into_value() {
			Primitive(p) => p.into_db_value(),
			Sequence (s)  =>
				flatten_iter(s.into_items().into_iter().map(InMemDicomObject::into_db_value)),
			_ => {todo!()}
		}
	}
}

impl<T> IntoDbValue for Option<T> where T:IntoDbValue
{
	fn into_db_value(self) -> sql::Value {
		self.map_or(sql::Value::None,T::into_db_value)
	}
}
