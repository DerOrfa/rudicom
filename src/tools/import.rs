use crate::db::{Entry, RecordId, RegisterResult};
use crate::tools::Error;
use futures::{Stream, StreamExt, TryStreamExt};
use glob::glob;
use itertools::Itertools;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::path::PathBuf;
use tokio::task::JoinError;

pub enum ImportResult {
	Registered{filename:String},
	Existed{filename:String,existing_id:RecordId},
	DataConflict {filename:String,existed:Entry},
	Md5Conflict {filename:String,existing_md5:String,my_md5:String, existing_id:RecordId},
	Err{filename:String,error:Error},
	GlobError(glob::GlobError)
}
#[derive(Clone,Deserialize)]
pub struct ImportConfig {
	#[serde(default)]
	pub echo:bool,
	#[serde(default)]
	pub echo_existing:bool,
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
			ImportResult::Existed { filename, existing_id:existed} => {
				let mut s=s.serialize_struct("existed",2)?;
				s.serialize_field("filename",filename)?;
				s.serialize_field("existing entry", existed.str_path().as_str())?;
				s.end()
			}
			ImportResult::DataConflict { filename, existed } => {
				let mut s=s.serialize_struct("existed_with_conflicting_data",3)?;
				s.serialize_field("existing path",existed.id().str_path().as_str())?;
				s.serialize_field("existing entry", &serde_json::Value::from(existed.clone()))?;
				s.serialize_field("filename",filename)?;
				s.end()
			}
			ImportResult::Md5Conflict {filename, existing_id,existing_md5,my_md5} => {
				let mut s=s.serialize_struct("existed_with_conflicting_checksum",3)?;
				s.serialize_field("existing_path",existing_id.str_path().as_str())?;
				s.serialize_field("filename",filename)?;
				s.serialize_field("incoming md5", my_md5)?;
				s.serialize_field("existing md5", existing_md5)?;
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
	let filename = path.display().to_string();
	let import = 
		match mode {
			ImportMode::Import => {crate::tools::store::import_file(path.as_path()).await}
			ImportMode::Store => {crate::tools::store::store_file(path.as_path()).await}
			ImportMode::Move => {crate::tools::store::move_file(path.as_path()).await}
		};
	match import
	{
		Ok(RegisterResult::Stored(_)) => ImportResult::Registered{ filename },
		Ok(RegisterResult::AlreadyStored(existed)) =>
			ImportResult::Existed {filename,existing_id:existed},
		Err(Error::Md5Conflict {existing_md5,my_md5, existing_id}) =>  
			ImportResult::Md5Conflict {filename,existing_md5,my_md5,existing_id},
		Err(Error::DataConflict(existed)) => 
			ImportResult::DataConflict { filename, existed },
		Err(e) => ImportResult::Err{error:e,filename},
	}
}


pub fn import_glob<T>(pattern:T, config:ImportConfig, mode: ImportMode) -> crate::tools::Result<impl Stream<Item=Result<ImportResult,JoinError>>> where T:AsRef<str>
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
				ImportResult::Existed { filename, existing_id } => {
					Ok(format!("{filename} already existed as {}", existing_id.str_path()))
				},
				ImportResult::DataConflict { filename, existed } => {
					match &existed {
						Entry::Instance(_) => {
							existed.get_file().map(|f|f.get_path())
								.map(|p| format!("{filename} was rejected as {} (in file {}) already exists but its values differ", existed.id().str_path(),p.display()))
								.map_err(|e|e.context(format!("Failed to extract information of existing entry of {filename}")))
						}
						_ => Ok(format!("{filename} was rejected as {} already exists but its values differ", existed.id().str_path()))
					}
				},
				ImportResult::Md5Conflict { filename, existing_id,.. } => 
					Ok(format!("{filename} was rejected as {} already exists but its checksum differs", existing_id.str_path())),
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
