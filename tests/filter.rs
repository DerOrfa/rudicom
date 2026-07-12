mod common;

use crate::common::dcm::cleanup;
use crate::common::{dcm, init_db};
use dicom::core::value::Value;
use dicom::dictionary_std::tags;
use dicom::object::AccessError;
use dimse::Taker;
use pyo3::types::PyModule;
use pyo3::Python;
use rudicom::db::{lookup, LocalSession, RegisterResult, Session, DB};
use rudicom::tools;
use rudicom::tools::store::store_ob;
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
//	tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();
	let code = CString::new(REPLACE_TIME)?;
	let mut obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1).into_inner();
	assert_eq!(String::get(&obj, tags::STUDY_DATE)?, "20250101");
	assert_eq!(String::get(&obj, tags::STUDY_TIME)?, "000000.000000");

	Python::attach(|py|{
		let code = PyModule::from_code(py, code.as_ref(), c"test.py", c"test")?;
		tools::filter::filter(code,&mut obj)
	})?;

	assert_eq!(String::get(&obj, tags::STUDY_DATE)?, "20250102");
	assert_eq!(String::get(&obj, tags::STUDY_TIME)?, "000010.000000");

	Ok(())
}

#[test]
fn missing_input()  -> Result<(), Box<dyn std::error::Error>>
{
//	tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();
	let code = CString::new(MISSING_INPUT)?;
	let mut obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1).into_inner();
	Python::attach(|py|{
		let code = PyModule::from_code(py, code.as_ref(), c"test.py", c"test")?;
		// first check with name existing
		tools::filter::filter(code.clone(),&mut obj)?;
		assert_eq!(String::take(&mut obj, tags::PATIENT_NAME)?, "Hello World");
		// removed name, check again
		if let Err(PythonErr(e)) = tools::filter::filter(code, &mut obj){
			assert_eq!(e.to_string(), "KeyError: 'input is missing'");
		} else { unreachable!(); }
		Ok(())
	})
}

#[tokio::test]
async fn filtered_store()  -> Result<(), Box<dyn std::error::Error>>
{
//	tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).init();
	init_db().await?.health().await?;
	let mut sess = LocalSession::create(&DB, 1);
	let mut obj = dcm::synthesize_series(&dcm::UidSynthesizer::default(), 1, 1, 2);

	if let RegisterResult::Stored(stored) = store_ob(obj.remove(0), &mut sess).await? {
		let stored = lookup(&stored).await?.expect("existing object should be found");
		let red = stored.get_file()?.read().await?;
		assert_eq!(stored.id().str_key(), red.element(tags::SOP_INSTANCE_UID)?.string()?);
		assert_eq!("000000.000000 ", red.element(tags::SERIES_TIME)?.string()?);
		assert_eq!("20250101", red.element(tags::SERIES_DATE)?.string()?);

		tools::remove::remove(stored.id()).await?;
	}
	else { panic!("Store should return stored."); }

	assert!(obj[0].update_value(tags::MODALITY,|v|*v = Value::from("SR")));
	if let RegisterResult::Stored(stored) = store_ob(obj.remove(0), &mut sess).await? {
		let stored = lookup(&stored).await?.expect("existing object should be found");
		let red = stored.get_file()?.read().await?;
		assert_eq!(stored.id().str_key(), red.element(tags::SOP_INSTANCE_UID)?.string()?);
		if let Err(AccessError::NoSuchDataElementTag { .. }) = red.element(tags::SERIES_DATE){}
		else { panic!("SERIES_DATE shouldn't be found"); };
		if let Err(AccessError::NoSuchDataElementTag { .. }) = red.element(tags::SERIES_TIME){}
		else { panic!("SERIES_TIME shouldn't be found"); };

		tools::remove::remove(stored.id()).await?;
	}
	else { panic!("Store should return stored."); }


	cleanup().await.map_err(|e| e.into())
}