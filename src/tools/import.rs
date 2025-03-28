use crate::db::Entry;
use crate::tools::Error;
use futures::{Stream, StreamExt, TryStreamExt};
use glob::glob;
use itertools::Itertools;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::path::PathBuf;
use tokio::task::JoinError;

pub(crate) enum ImportResult {
	Registered{filename:String},
	Existed{filename:String,existed:Entry},
	ExistedConflict {filename:String,my_md5:String,existed:Entry},
	Err{filename:String,error:Error},
	GlobError(glob::GlobError)
}
#[derive(Clone,Deserialize)]
pub(crate) struct ImportConfig {
	#[serde(default)]
	pub(crate) echo:bool,
	#[serde(default)]
	pub(crate) echo_existing:bool,
}

#[derive(clap::ValueEnum, Clone, Default, Debug, Serialize, Copy)]
pub enum ImportMode{
	/// won't touch or own the file, but register it in the DB
	#[default]
	Import,
	/// won't touch the file but create an owned copy inside the configured storage path (which might collide with the source file)
	Store,
	/// if the source is inside the configured storage path the DB takes ownership of the existing file, otherwise file will be moved into the configured storage path
	Move
}

impl ToString for ImportMode
{
	fn to_string(&self) -> String {
		match self {
			ImportMode::Import => "import",
			ImportMode::Store => "store",
			ImportMode::Move => "move"
		}.to_string()
	}
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
			},
			ImportResult::GlobError(error) => {
				let mut s=s.serialize_struct("failed",2)?;
				s.serialize_field("path",error.path())?;
				s.serialize_field("error", error.to_string().as_str())?;
				s.end()
			}
		}
	}
}

async fn import_file<T>(path:T, mode: ImportMode) -> ImportResult where T:Into<PathBuf>
{
	let path= path.into();
	let filename = path.to_string_lossy().to_string();
	let import = 
		match mode {
			ImportMode::Import => {crate::tools::store::import_file(path.as_path()).await}
			ImportMode::Store => {crate::tools::store::store_file(path.as_path()).await}
			ImportMode::Move => {crate::tools::store::move_file(path.as_path()).await}
		};
	match import
	{
		Ok(v) => match v {
			None => ImportResult::Registered{ filename },
			Some(mut existed) => {
				if let Some(conflicting_md5) = existed.remove("conflicting_md5"){
					ImportResult::ExistedConflict {
						filename,existed,
						my_md5: conflicting_md5.into_inner().as_raw_string().to_string()
					}
				} else {ImportResult::Existed { filename, existed }}
			},
		},
		Err(e) => ImportResult::Err{error:e,filename}
	}
}


pub(crate) fn import_glob<T>(pattern:T, config:ImportConfig, mode: ImportMode) -> crate::tools::Result<impl Stream<Item=Result<ImportResult,JoinError>>> where T:AsRef<str>
{
	let mut tasks=tokio::task::JoinSet::new();
	let mut files= glob(pattern.as_ref())?.filter_map_ok(|p|
		if p.is_file() {Some(p)} else {None}
	);

	//if there is not at least one file, it's probably a good idea to return an error
	if let Some(file)=files.next().transpose()?{
		tasks.spawn(import_file(file,mode));
	} else {
		return Err(Error::NotFound.context(format!("when looking for files in {}",pattern.as_ref())))
	}
	// make a stream that polls tasks and feeds new ones
	let stream=futures::stream::poll_fn(move |c|{
		// fill task list up to max_files 
		while tasks.len() < crate::config::get().limits.max_files as usize
		{
			if let Some(nextfile) = files.next() {
				tasks.spawn(async move {
					match nextfile
					{
						Ok(p) => import_file(p, mode).await,
						Err(e) => ImportResult::GlobError(e.into())
					}
				});
			} else {break} //as long as there are files  
		}
		// pass on next finished import and thus drain the task list
		tasks.poll_join_next(c)
	});
	let stream= stream.filter(move |item|{
			let ret =	match item.to_owned() {
				Ok(ImportResult::Registered { .. }) => config.echo,
				Ok(ImportResult::Existed { .. }) => config.echo_existing,
				_ => true
			};
			async move {ret}
		});
	Ok(stream)
}

pub fn import_glob_as_text<T>(pattern:T, config:ImportConfig, mode: ImportMode) -> crate::tools::Result<impl Stream<Item=Result<String,JoinError>>> where T:AsRef<str>
{
	Ok(import_glob(pattern, config, mode)?
		.map_ok(|item| {
			let register_msg = match item {
				ImportResult::Registered { filename } => Ok(filename),
				ImportResult::ExistedConflict { filename, existed, .. } => {
					existed.get_file().map(|f|f.get_path())
						.map(|p| format!("{filename} already existed as {} but checksum differs", p.to_string_lossy()))
						.map_err(|e|e.context(format!("Failed to extract information of existing entry of {filename}")))
				},
				ImportResult::Existed { filename, existed } => {
					existed.get_file().map(|f|f.get_path())
						.map(|p| format!("{filename} already existed as {}", p.to_string_lossy()))
						.map_err(|e|e.context(format!("Failed to extract information of existing entry of {filename}")))
				},
				ImportResult::Err { filename, error } => {
					Err(error.context(format!("importing {filename}")))
				}
				ImportResult::GlobError(e) => Err(e.into())
			};
			register_msg.unwrap_or_else(|e|
				String::from("E:")+e.sources().join("\nE:>").as_str()
			)
		})
	)
}
