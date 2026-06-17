use super::error::Result;
use dicom::object::mem::InMemElement;
use std::io::{Cursor, Read};
use std::str::FromStr;
use dicom::core::PrimitiveValue;
use dicom::core::value::C;
use tokio_util::bytes::Buf;
use tracing::{debug, trace};
use crate::tools::Error::{DicomError, ParseError};

pub fn extract_csa(el:&InMemElement, name:&str) -> Result<Option<PrimitiveValue>>{
	let bytes = el.to_bytes().map_err(|e|DicomError(e.into()))?;
	let mut cursor= Cursor::new(bytes);
	cursor.advance(0x10);
	while cursor.remaining() > 0x40 {
		if let Some(entry) = extract_csaentry(&mut cursor, name)?
		{
			return Ok(Some(entry));
		}
	}
	Ok(None)
}

fn extract_csaentry<T:AsRef<[u8]>>(cursor:&mut Cursor<T>, lookup:&str) -> Result<Option<PrimitiveValue>>{
	let name = get_string(cursor, 0x40).expect("Invalid Name in CSA");
	let _vm = cursor.get_u32_le();
	let vr = get_string(cursor, 0x4).expect("Invalid Name in CSA");
	let _syngodt = cursor.get_i32_le();
	let nitems = cursor.get_i32_le();
	let _77 = cursor.get_i32_le();
	let skip = name != lookup;

	if nitems == 0 { return Ok(None); }
	for _n in 0..nitems {
		let mut len = cursor.get_i32_le();
		cursor.advance(12); //whatever
		if len == 0 {continue}
		len = ((len+3) / 4) * 4; // padding
		if !skip {
			let insert = get_string(cursor, len as usize).expect("Invalid Value in CSA");
			let words = insert.split_ascii_whitespace().collect::<Vec<_>>();
			if words.is_empty() {
				debug!("Skipping empty string for CSA entry ");
				continue
			} else {
				let val = parse_csa_values(words, vr.as_str())?;
				debug!("Found scalar entry {name}:{val} in CSA header");
				return Ok(Some(val));
			}
		} else {
			trace!("Skipping item {_n} in {name} as its not what we're looking for");
			cursor.advance(len as usize);
		}
	}
	Ok(None)
}

fn convert_vec<T,P,E>(words:Vec<&str>, proc:P) -> Result<C<T>>
	where
		E:std::error::Error + Send + Sync + 'static,
		P:Fn(&str) -> std::result::Result<T,E>,
{
	words.into_iter()
		.map(|w|proc(w).map_err(|e|ParseError { to_parse:w.to_string() , source: Box::new(e) }))
		.collect::<std::result::Result<Vec<_>,_>>()
		.map(|v|v.into())
}
fn parse_csa_values(values: Vec<&str>, vr: &str) -> Result<PrimitiveValue> {
	Ok(match vr {
		"IS"|"SL" => PrimitiveValue::I32(convert_vec(values,i32::from_str)?),
		"UL" => PrimitiveValue::U32(convert_vec(values,u32::from_str)?),
		"CS"|"LO"|"SH"|"UN"|"ST"|"UT"|"LT"|"UR" => PrimitiveValue::Strs(convert_vec(values,String::from_str)?),
		"OD"|"DS"|"FD" => PrimitiveValue::F64(convert_vec(values,f64::from_str)?),
		"US"|"OW" => PrimitiveValue::U16(convert_vec(values,u16::from_str)?),
		"SS" => PrimitiveValue::I16(convert_vec(values,i16::from_str)?),
		_ => panic!("Don't know how to parse CSA entry")
	})
}

fn get_string<T:AsRef<[u8]>>(cursor:&mut Cursor<T>, size:usize) -> Result<String>{
	let mut name = Vec::new();
	name.resize(size, 0);
	cursor.read(&mut name)?;
	name.truncate(name.iter().position(|&b| b==0).expect("Invalid String in CSA"));
	Ok(String::from_utf8(name).expect("Invalid String in CSA"))
}