use std::collections::BTreeMap;
use dicom::core::{DataDictionary, PrimitiveValue};
use dicom::core::chrono;
use dicom::object::mem::InMemElement;
use dicom::core::value::Value::{Primitive,Sequence};
use dicom::object::{InMemDicomObject, StandardDataDictionary};
use crate::db::DbVal;

fn flatten_iter<T,I>(iter:I) -> DbVal
	where I:Iterator<Item=T>, T:Into<DbVal>, T:Clone
{
	let mut vec:Vec<DbVal> = iter.map(|v|v.into()).collect();
	match vec.len() {
		0 => DbVal::None,
		1 => vec.pop().unwrap(),
		_ => vec.into()
	}
}

pub trait IntoDbValue{
	fn into_db_value(self) -> DbVal;
}

impl IntoDbValue for InMemDicomObject{
	fn into_db_value(self) -> DbVal {
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
	fn into_db_value(self) -> DbVal {
		use PrimitiveValue::*;
		match self {
			Empty => DbVal::None, // no-op
			Date(dates) => flatten_iter(dates.into_iter().
				map(|date|
					date.to_naive_date().unwrap()
						.and_time(chrono::NaiveTime::default())
						.and_utc()
				)
			),
			Time(times) => flatten_iter(times.into_iter()
				.map(|time|
					chrono::NaiveDate::default()
						.and_time(time.to_naive_time().unwrap())
						.and_utc()
				)
			),
			DateTime(datetimes) => flatten_iter(datetimes.into_iter()
				.map(|datetime|
					datetime.to_chrono_datetime().unwrap().with_timezone(&chrono::Utc)
				)
			),
			Str(s) => s.into(),
			Strs(s) => flatten_iter(s.into_iter()),
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
	fn into_db_value(self) -> DbVal {
		match self.into_value() {
			Primitive(p) => p.into_db_value(),
			Sequence { items: objects, ..}  =>
				flatten_iter(objects.into_iter().map(|o|o.clone().into_db_value())),
			_ => {todo!()}
		}
	}
}

impl<T> IntoDbValue for Option<T> where T:IntoDbValue{
	fn into_db_value(self) -> DbVal {
		self.map_or(DbVal::None,|v|v.into_db_value())
	}
}
