use std::collections::HashMap;
use std::ffi::CStr;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use dicom::core::header::Header;
use dicom::core::{DataDictionary, PrimitiveValue, Tag, VR};
use dicom::core::dictionary::VirtualVr;
use dicom::core::smallvec::SmallVec;
use dicom::core::value::{DicomDate, DicomDateTime, DicomTime, Value, C};
use dicom::dictionary_std::StandardDataDictionary;
use dicom::object::InMemDicomObject;
use dicom::object::mem::InMemElement;
use pyo3::{Bound, IntoPyObject, PyAny, PyErr, Python};
use pyo3::conversion::FromPyObjectOwned;
use pyo3::exceptions::PyValueError;
use pyo3::types::{PyAnyMethods, PyDict, PyModule};
use tracing::{debug, error};
use crate::tools;

fn auto_array<'py, T>(val: Bound<'py, PyAny>) -> Result<C<T>, PyErr> where T:FromPyObjectOwned<'py>, T:Clone
{
	if let Some(val) = val.extract::<Vec<T>>().ok(){
		Ok(val.into_iter().collect())
	} else 	if let Some(val) = val.extract::<T>().ok(){
		Ok(SmallVec::from_elem(val,1))
	} else { Err(PyValueError::new_err(format!("cannot extract {val}"))) }
}

fn edit_value(val: Bound<PyAny>, e:&mut PrimitiveValue) -> Result<(), PyErr>
{
	match e {
		PrimitiveValue::Strs(s) => auto_array(val).map(|v|{*s=v;()}),
		PrimitiveValue::Str(s) => val.extract::<String>().map(|v|{*s = v;()}),
		PrimitiveValue::U8(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::I16(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::U16(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::I32(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::U32(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::I64(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::U64(i) => auto_array(val).map(|v|{*i=v;()}),
		PrimitiveValue::F32(f) => auto_array(val).map(|v|{*f=v;()}),
		PrimitiveValue::F64(f) => auto_array(val).map(|v|{*f=v;()}),
		PrimitiveValue::Date(d) => {
			let date = DicomDate::try_from(&val.extract::<NaiveDate>()?)
				.map_err(|e| PyErr::new::<PyValueError, _>(format!("{}", e)))?;
			*d = SmallVec::from_elem(date,1);
			Ok(())
		}
		PrimitiveValue::DateTime(d) => {
			let date = DicomDateTime::try_from(&val.extract::<NaiveDateTime>()?)
				.map_err(|e| PyErr::new::<PyValueError, _>(format!("{e}")))?;
			*d = SmallVec::from_elem(date,1);
			Ok(())
		}
		PrimitiveValue::Time(t) => {
			let time = DicomTime::try_from(&val.extract::<NaiveTime>()?)
				.map_err(|e| PyValueError::new_err(format!("{e}")))?;
			*t = SmallVec::from_elem(time,1);
			Ok(())
		}
		_ => Err(PyValueError::new_err(format!("cannot extract {val}")))
	}
}

fn replace_element(val: Bound<PyAny>, e:InMemElement) -> Result<InMemElement, PyErr>
{
	let tag = e.tag();
	let vr = e.vr();
	let err = format!("{} is not a primitive value", e.to_str().unwrap());
	if let Some(mut e_val) = e.into_value().into_primitive(){
		edit_value(val,&mut e_val)?;
		Ok(InMemElement::new(tag,vr,e_val))
	} else { Err(PyValueError::new_err(err)) }
}

fn make_value(val: Bound<PyAny>, vr:VR) -> Result<PrimitiveValue, PyErr> {
	match vr {
		VR::AE | VR::AS | VR::CS | VR::DA | VR::DS | VR::DT | VR::IS | VR::LO | VR::LT | VR::OD |
		VR::OF | VR::OW | VR::PN | VR::SH | VR::ST | VR::TM | VR::UC | VR::UI | VR::UR | VR::UT =>
			{ // String
				if let Some(val) = val.extract::<Vec<String>>().ok(){
					Ok(PrimitiveValue::Strs(val.into()))
				} else 	if let Some(val) = val.extract::<String>().ok(){
					Ok(PrimitiveValue::Str(val))
				} else { Err(PyValueError::new_err(format!("cannot extract {val}"))) }
			}
		VR::FL => Ok(PrimitiveValue::F32(auto_array(val)?)),
		VR::FD => Ok(PrimitiveValue::F64(auto_array(val)?)),
		VR::SS => Ok(PrimitiveValue::I16(auto_array(val)?)),
		VR::SL => Ok(PrimitiveValue::I32(auto_array(val)?)),
		VR::SV => Ok(PrimitiveValue::I64(auto_array(val)?)),
		VR::US => Ok(PrimitiveValue::U16(auto_array(val)?)),
		VR::OL | VR::UL => Ok(PrimitiveValue::U32(auto_array(val)?)),
		VR::OV | VR::UV => Ok(PrimitiveValue::U64(auto_array(val)?)),
		_ => Err(PyValueError::new_err(format!("cannot extract {val} as {}", vr.to_string())))
	}
}

fn auto_unarray<'a, T,A,E>(v:C<T>,py:Python<'a>) -> Result<Bound<'a,PyAny>,PyErr>
	where T:IntoPyObject<'a, Output = Bound<'a, A>, Error = E> + Clone,	PyErr:From<E>
{
	if v.len() != 1 {v.into_vec().into_pyobject(py)}
	else {
		v[0].clone().into_pyobject(py)
			.map(|v|v.into_any())
			.map_err(PyErr::from)
	}
}
fn from_primitive(v:PrimitiveValue, py:Python) -> Result<Bound<PyAny>, PyErr> {
	match v {
		PrimitiveValue::Empty => None::<()>.into_pyobject(py).map_err(PyErr::from),
		PrimitiveValue::Strs(s) => s.into_pyobject(py),
		PrimitiveValue::Str(s) => Ok(s.into_pyobject(py)?.into_any()),
		PrimitiveValue::U8(v) => auto_unarray(v,py),
		PrimitiveValue::I16(v) => auto_unarray(v,py),
		PrimitiveValue::U16(v) => auto_unarray(v,py),
		PrimitiveValue::I32(v) => auto_unarray(v,py),
		PrimitiveValue::U32(v) => auto_unarray(v,py),
		PrimitiveValue::I64(v) => auto_unarray(v,py),
		PrimitiveValue::U64(v) => auto_unarray(v,py),
		PrimitiveValue::F32(v) => auto_unarray(v,py),
		PrimitiveValue::F64(v) => auto_unarray(v,py),
		// @todo handle fails
		PrimitiveValue::Date(v) => {
			let v:SmallVec<_> = v.into_iter().map(|v|v.to_naive_date().unwrap()).collect();
			auto_unarray(v,py)
		}
		PrimitiveValue::DateTime(v) => {
			let v:SmallVec<_> = v.into_iter()
				.map(|v|v.to_precise_datetime().unwrap().as_naive_datetime().unwrap().clone()).collect();
			auto_unarray(v,py)
		}
		PrimitiveValue::Time(v) => {
			let v:SmallVec<_> = v.into_iter().map(|v|v.to_naive_time().unwrap()).collect();
			auto_unarray(v,py)
		}
		_ => Err(PyValueError::new_err(format!("cannot make a python value from {v}")))
	}
}
fn make_element(val: Bound<PyAny>, tag: Tag) -> Result<InMemElement, PyErr> {
	if let VirtualVr::Exact(vr) = StandardDataDictionary::default().by_tag(tag).unwrap().vr{
		make_value(val,vr).map(|v|InMemElement::new(tag,vr,v))
	} else { Err(PyValueError::new_err(format!("Invalid VR for tag {}", tag.to_string()))) }
}

pub fn filter(code:&CStr, obj:&mut InMemDicomObject, py:Python) -> tools::Result<()>
{
	let module = PyModule::from_code(py, code, c"", c"test")?;
	let input = module.getattr_opt("input_tags")?
		.map(|i|i.extract::<Vec<(u16,u16)>>()).transpose()?
		.unwrap_or_default();
	let input = input.into_iter().map(Tag::from)
		.filter_map(|tag| obj.element(tag).map(|e| (tag, e)).ok())
		.collect::<HashMap<_,_>>();
	let param = PyDict::new(py);
	for (t,e) in input
	{
		if let Value::Primitive(val) = e.value() {
			param.set_item((t.0,t.1), from_primitive(val.clone(),py)?)?;
		} else {
			error!("Ignoring {} as it is not a primitive value", e.to_str().unwrap());
		}
	}
	let res:HashMap<(u16,u16),Option<Bound<PyAny>>> = module
		.call_method1("filter", (param,))?.extract()?;
	for (tag,val) in res {
		let tag = Tag::from(tag);
		if let Some(val) = val {
			let new_e = if let Some(e) = obj.take_element(tag).ok() {
				replace_element(val, e)?
			} else { make_element(val, tag)?};
			debug!("Replacing {} with {}", tag.to_string(), new_e.to_str().unwrap());
			obj.put_element(new_e);
		} else {
			debug!("Removing {}", tag.to_string());
			obj.remove_element(tag);
		}
	}
	Ok(())
}