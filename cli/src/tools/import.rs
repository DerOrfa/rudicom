use std::path::Path;
use tokio::task::JoinError;
use futures::{Stream, TryStreamExt, StreamExt};
use glob::glob;
use itertools::Itertools;
use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;
use crate::db::{Entry, File};
use crate::tools::Error;
use crate::tools::store::import;

pub(crate) enum ImportResult {
	Registered{filename:String},
	Existed{filename:String,existed:Entry},
	ExistedConflict {filename:String,my_md5:String,existed:Entry},
	Err{filename:String,error:Error}
}

impl Serialize for ImportResult
{
	fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> where S: Serializer {
		match self {
			ImportResult::Registered { filename } => {
				let mut s=s.serialize_struct("registered",1)?;
				s.serialize_field("filename",filename)?;
				s.end()
			}
			ImportResult::ExistedConflict {existed, filename, my_md5  } => {
				let mut s=s.serialize_struct("existed_with_conflicting_checksum",3)?;
				let existed = serde_json::Value::from(existed.clone());
				s.serialize_field("filename",filename)?;
				s.serialize_field("incoming_md5", my_md5)?;
				s.serialize_field("existing entry", &existed)?;
				s.end()
			}
			ImportResult::Existed { filename,existed } => {
				let mut s=s.serialize_struct("existed",2)?;
				let existed = serde_json::Value::from(existed.clone());
				s.serialize_field("filename",filename)?;
				s.serialize_field("existing entry", &existed)?;
				s.end()
			}
			ImportResult::Err { filename,error} => {
				let mut s=s.serialize_struct("failed",2)?;
				s.serialize_field("filename", filename)?;
				s.serialize_field("error", error.to_string().as_str())?;
				let chain:Vec<_>= error.sources().map(|e|e.to_string()).collect();
				if chain.len()>0 {
					s.serialize_field("causation",&chain)?;
				}
				s.end()
			}
		}
	}
}

async fn import_file<T>(path:T) -> ImportResult where T:AsRef<Path>
{
	let filename= path.as_ref().to_string_lossy().to_string();
	match import(path.as_ref()).await
	{
		Ok(v) => match v {
			None => ImportResult::Registered{ filename },
			Some(mut existed) => {
				if let Some(conflicting_md5) = existed.remove("conflicting_md5"){
					ImportResult::ExistedConflict {
						filename,existed,
						my_md5: conflicting_md5.as_raw_string().to_string()
					}
				} else {ImportResult::Existed { filename, existed }}
			},
		},
		Err(e) => {
			ImportResult::Err{
				error:e.context(format!("registering {} failed",path.as_ref().to_string_lossy())),
				filename
			}
		}
	}
}


pub(crate) fn import_glob<T>(pattern:T, report_registered:bool,report_existing:bool) -> crate::tools::Result<impl Stream<Item=Result<ImportResult,JoinError>>> where T:AsRef<str>
{
	let mut tasks=tokio::task::JoinSet::new();
	let mut files= glob(pattern.as_ref())
		.map_err(|e|Error::GlobbingError{pattern:pattern.as_ref().to_string(),err:e})?
		.filter_map(|f|
			if let Ok(f)=f{
				if f.is_file(){Some(f)} else {None}
			} else {None}
		);

	//pre-fill first 10 register tasks so there will always be some tasks that can do stuff
	//also if there is not at least one file, it's probably a good idea to return an error
	if let Some(file)=files.next(){
		tasks.spawn(import_file(file));
	} else {
		return Err(Error::NotFound.context(format!("when looking for files in {}",pattern.as_ref())))
	}
	for _ in 1..9{
		files.next().map(|nextfile|
			tasks.spawn(import_file(nextfile)));
	}
	// make a stream that polls tasks and feeds new ones
	let stream=futures::stream::poll_fn(move |c|{
		let p=tasks.poll_join_next(c);
		if p.is_ready() {//if a task finished
			// spawn a new one, if there are still files in the globber
			files.next().map(|nextfile|
				tasks.spawn(import_file(nextfile)));
		}
		p // then just send the Poll along for the stream to deal with
	});
	let stream= stream.filter(move |item|{
			let ret= if let Ok(stored) = item.to_owned() {
				match stored {
					ImportResult::Registered { .. } => report_registered,
					ImportResult::ExistedConflict { .. } => true,
					ImportResult::Existed { .. } => report_existing,
					ImportResult::Err { .. } => true
				}
			} else {true};
			async move {ret}
		});
	Ok(stream)
}

pub fn import_glob_as_text<T>(pattern:T, report_registered:bool,report_existing:bool) -> crate::tools::Result<impl Stream<Item=Result<String,JoinError>>> where T:AsRef<str>
{
	Ok(import_glob(pattern,report_registered,report_existing)?
		.map_ok(|item| {
			let register_msg = match item {
				ImportResult::Registered { filename } => Ok(format!("{filename} stored")),
				ImportResult::ExistedConflict { filename, existed, .. } => {
					File::try_from(existed).map(|f|f.get_path())
						.map(|p| format!("{filename} already existed as {} but checksum differs", p.to_string_lossy()))
						.map_err(|e|e.context(format!("Failed to extract information of existing entry of {filename}")))
				},
				ImportResult::Existed { filename, existed } => {
					File::try_from(existed).map(|f|f.get_path())
						.map(|p| format!("{filename} already existed as {}", p.to_string_lossy()))
						.map_err(|e|e.context(format!("Failed to extract information of existing entry of {filename}")))
				},
				ImportResult::Err { filename, error } => {
					Err(error.context(format!("Importing {filename} failed")))
				}
			};
			register_msg.unwrap_or_else(|e|
				String::from("E:")+e.sources().join("\nE:>").as_str()
			)
		})
	)
}
