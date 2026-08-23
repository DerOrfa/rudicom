use crate::db::register_manager::RegisterManager;
use crate::db::{Entry, RecordId, RegisterResult};
use crate::tools::Error;
use crate::{storage, tools};
use futures::{Stream, StreamExt, TryStreamExt, stream};
use glob::glob;
use itertools::Itertools;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::Display;
use std::io::ErrorKind;
use dicom::object::from_reader;
use tokio::task::spawn_blocking;
use crate::tools::Error::DicomError;

pub enum ImportResult {
	Registered { filename: String },
	Existed { filename: String, existing_id: RecordId },
	DataConflict { filename:String,existed:Entry },
	FieldConflict { filename:String,id:RecordId, fields:String },
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
	/// won't touch the file but create an owned 1 to 1 copy inside the configured storage path (which might collide with the source file)
	Copy,
	/// won't touch the file but process (and possibly modify) the data and store it inside the configured storage path (which might collide with the source file)
	Store,
	/// Like Store but moves the file into the configured storage path (if it's already there, DB just takes ownership)
	Move
}

impl Display for ImportMode
{
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let str = match self {
			ImportMode::Import => "import",
			ImportMode::Copy => "copy",
			ImportMode::Store => "store",
			ImportMode::Move => "move"
		};
		f.write_str(str)
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
			ImportResult::FieldConflict { filename, id, fields } => {
				let mut s=s.serialize_struct("existed_with_conflicting_fields",3)?;
				s.serialize_field("existing path", id.str_path().as_str())?;
				s.serialize_field("conflicting fields", fields)?;
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

fn process_register_error(res:tools::Error,path:impl ToString) -> ImportResult
{
	let filename = path.to_string();
	match res
	{
		Error::Md5Conflict {existing_md5,my_md5, existing_id} =>
			ImportResult::Md5Conflict {filename,existing_md5,my_md5,existing_id},
		Error::DataConflict(existed) =>
			ImportResult::DataConflict { filename, existed },
		Error::FieldConflict{ fields, id } =>
			ImportResult::FieldConflict { filename, id, fields },
		e => ImportResult::Err{error:e,filename},
	}
}

pub fn import_glob<T>(pattern:T, config:ImportConfig, mode: ImportMode) -> tools::Result<impl Stream<Item=ImportResult>> where T:AsRef<str>
{
	let max_files = crate::config::get().limits.max_files;
	let mut files= glob(pattern.as_ref())?.filter_map_ok(|p|
		if p.is_file() {Some(p)} else {None}
	);
	let manager = RegisterManager::new();

	// if there is not at least one file, it's probably a good idea to return an error
	if let Some(file)=files.next().transpose()? {
		let files = [Ok(file)].into_iter().chain(files);
		let stream = stream::iter(files)
			.map_err(ImportResult::GlobError)
			.map_ok(move|p|async move {
				let filename = p.to_string_lossy().to_string();
				match mode {
					ImportMode::Import => storage::Image::from_existing(p).await,
					ImportMode::Copy => storage::Image::copy_existing(p).await,
					ImportMode::Store => {
						spawn_blocking(||from_reader(std::fs::File::open(p)?)
								.map_err(|e|DicomError(e.into()))
						).await.map_err(Error::from).flatten()
							.and_then(storage::Image::from_obj_filtered)
					},
					ImportMode::Move => storage::Image::move_existing(&p).await,
				}
				.map(|i|(filename.clone(),i))
				.map_err(|e|process_register_error(e,filename))
			})
			.try_buffer_unordered(max_files as usize) // load the images (parallel)
			.and_then(move|(path,image)|{
				let mut shared_manager = manager.clone();
				async move { // feed the queue
					match shared_manager.register(image).await
					{
						Ok(r) => Ok((path.clone(),r)),
						Err(e) => Err(process_register_error(e,path.clone()))
					}
				}
			})
			.map_ok(|(filename,receiver)|async { // listen for results
				match receiver.await.unwrap_or_else(|e|Err(Error::IoError(std::io::Error::new(ErrorKind::BrokenPipe,e))))
				{
					Ok(RegisterResult::Stored(_)) => Ok(ImportResult::Registered{ filename }),
					Ok(RegisterResult::AlreadyStored(existed)) =>
						Ok(ImportResult::Existed {filename,existing_id:existed}),
					Err(e) => Err(process_register_error(e,filename)),
				}
			})
			.try_buffer_unordered(max_files as usize) // load the images (parallel)
			.try_filter_map(move |item|{
				async move {
					if match &item {
						ImportResult::Registered { .. } => config.echo,
						ImportResult::Existed { .. } => config.echo_existing,
						_ => true
					}{Ok(Some(item))}else { Ok(None) }
				}
			})
			.map(|item|item.unwrap_or_else(|e|e));
		Ok(stream)
	} else {
		Err(Error::NotFound.context(format!("when looking for files in {}",pattern.as_ref())))
	}
}

pub fn import_glob_as_text<T>(pattern:T, config:ImportConfig, mode: ImportMode) -> tools::Result<impl Stream<Item=String>> where T:AsRef<str>
{
	Ok(import_glob(pattern, config, mode)?
		.map(|item| {
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
				ImportResult::FieldConflict { filename, id, fields } =>
					Ok(format!("{filename} was rejected as {id} already exists but its fields \"{}\" differ", fields)),
				ImportResult::Md5Conflict { filename, existing_id,.. } =>
					Ok(format!("{filename} was rejected as {} already exists but its checksum differs", existing_id.str_path())),
				ImportResult::Err { filename, error } => {
					Err(error.context(format!("importing {filename}")))
				}
				ImportResult::GlobError(e) => Err(e.into()),
			};
			register_msg.unwrap_or_else(|e|
				String::from("E:")+e.sources().join("\nE:>").as_str()
			)
		})
	)
}
