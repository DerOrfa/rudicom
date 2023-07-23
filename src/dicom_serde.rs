use dicom::core::PrimitiveValue;
use dicom::core::value::Value;
use dicom::object::mem::InMemElement;
use dicom::object::StandardDataDictionary;
use serde::{Serialize, Serializer};
use serde::ser::{SerializeSeq,Error};

pub struct ElementAdapter<'a,D=StandardDataDictionary>{
	data:&'a InMemElement<D>
}

impl<D> Serialize for ElementAdapter<'_,D> {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer
	{
		serialize_object(self.data,serializer)
	}
}

impl<'a,D> From<&'a InMemElement<D>> for ElementAdapter<'a,D>{
	fn from(value: &'a InMemElement<D>) -> Self {
		ElementAdapter {data:value}
	}
}

fn serialize_object<D,S>(obj:&InMemElement<D>,serializer:S) -> Result<S::Ok, S::Error>
	where S: Serializer,
{
	match obj.value() {
		Value::Primitive(prim) => {
			serialize_primitive(prim,serializer)
				.or(Err(Error::custom(format!("Failed parsing {}",obj.to_str().unwrap()))))
		},
		Value::Sequence { .. } => {
			let mut seq = serializer.serialize_seq(None).unwrap();

			for o in obj.items().unwrap(){
				for i in o.into_iter(){
					seq.serialize_element(&ElementAdapter::from(i))?;
				}
			}
			seq.end()
		}
		Value::PixelSequence { .. } => {todo!()}
	}
}

fn serialize_collection<S, T, U, F>(col: &[T], mapping: F,serializer:S) -> Result<S::Ok, S::Error>
	where S: Serializer, U: Serialize, F: Fn(&T) -> U
{
	match col.len() {
		0 => serializer.serialize_none(),
		1 => mapping(col.first().unwrap()).serialize(serializer),
		_ => {
			let mut seq = serializer.serialize_seq(Some(col.len())).unwrap();
			for e in col{
				seq.serialize_element(&mapping(&e))?;
			}
			seq.end()
		}
	}
}

fn serialize_primitive<S>(value: &PrimitiveValue, serializer:S ) -> Result<S::Ok, S::Error>
	where S: Serializer
{
	use PrimitiveValue::*;
	match value {
		Empty => serializer.serialize_none(), // no-op
		Date(date) =>
			serialize_collection(date, |date| date.to_naive_date().unwrap(), serializer),
		Time(time) =>
			serialize_collection(time, |time| time.to_naive_time().unwrap(), serializer),
		DateTime(datetime) =>
			serialize_collection(datetime, |datetime| datetime.to_chrono_datetime().unwrap(), serializer),
		Str(s) => s.serialize(serializer),
		Strs(s) => serialize_collection(s,|s|s.clone(),serializer),
		F32(values) => serialize_collection(values,|v|v.clone(),serializer),
		F64(values) => serialize_collection(values,|v|v.clone(),serializer),
		U64(values) => serialize_collection(values,|v|v.clone(),serializer),
		I64(values) => serialize_collection(values,|v|v.clone(),serializer),
		U32(values) => serialize_collection(values,|v|v.clone(),serializer),
		I32(values) => serialize_collection(values,|v|v.clone(),serializer),
		U16(values) => serialize_collection(values,|v|v.clone(),serializer),
		I16(values) => serialize_collection(values,|v|v.clone(),serializer),
		U8(values) => serialize_collection(values,|v|v.clone(),serializer),
		Tags(tags) => serialize_collection(tags,|t|t.to_string(),serializer)
	}
}
