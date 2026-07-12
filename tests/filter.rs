mod common;

use crate::common::dcm;
use dicom::dictionary_std::tags;
use dimse::Taker;
use pyo3::Python;
use rudicom::tools;
use rudicom::tools::Error::PythonErr;
use std::ffi::CString;

static REPLACE_TIME:&str = r#"
from typing import Any, Optional

def filter(input:dict[tuple[int,int],Any]) -> dict[tuple[int,int],Optional[Any]]:
	return {
		(0x0008,0x0020):"20250102",
		(0x0008,0x0030):"000010.000000",
	}
"#;

static MISSING_INPUT:&str = r#"
input_tags = [(0x0010,0x0010)]

def filter(input:dict[tuple[int,int],Any]) -> dict[tuple[int,int],Optional[Any]]:
	if (0x0010,0x0010) in input:
		return {(0x0010,0x0010):"Hello World"}
	else:
		raise KeyError("input is missing")
"#;

#[test]
fn replace_filter()  -> Result<(), Box<dyn std::error::Error>>
{
	tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();
	let code = CString::new(REPLACE_TIME)?;
	let mut obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1).into_inner();
	assert_eq!(String::get(&obj, tags::STUDY_DATE)?, "20250101");
	assert_eq!(String::get(&obj, tags::STUDY_TIME)?, "000000.000000");

	Python::attach(|py|tools::filter::filter(code.as_ref(),&mut obj, py))?;

	assert_eq!(String::get(&obj, tags::STUDY_DATE)?, "20250102");
	assert_eq!(String::get(&obj, tags::STUDY_TIME)?, "000010.000000");

	Ok(())
}

#[test]
fn missing_input()  -> Result<(), Box<dyn std::error::Error>>
{
	tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();
	let code = CString::new(MISSING_INPUT)?;
	let mut obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1).into_inner();
	// first check with name existing
	Python::attach(|py|tools::filter::filter(code.as_ref(),&mut obj, py))?;
	assert_eq!(String::take(&mut obj, tags::PATIENT_NAME)?, "Hello World");
	// removed name, check again
	if let Err(PythonErr(e)) = Python::attach(|py|tools::filter::filter(code.as_ref(), &mut obj, py)){
		assert_eq!(e.to_string(), "KeyError: 'input is missing'");
	} else { unreachable!(); }
	Ok(())
}
