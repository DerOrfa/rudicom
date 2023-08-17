use std::path::Path;
use anyhow::{Result,anyhow};
use serde_json::Map;
use tokio::task::JoinError;
use crate::JsonVal;
use crate::storage::register_file;
use futures::{Stream, TryStreamExt};
use glob::glob;

pub(crate) enum ImportResult {
	Registered{filename:String},
	Existed{filename:String,existed:Map<String,JsonVal>},
	Err{filename:String,error:anyhow::Error}
}
async fn import_file<T>(path:T) -> ImportResult where T:AsRef<Path>
{
	let filename= path.as_ref().to_string_lossy().to_string();
	match register_file(path.as_ref()).await{
		Ok(v) => match v {
			JsonVal::Null => ImportResult::Registered{ filename },
			JsonVal::Object(existed) => ImportResult::Existed{ filename,existed },
			_ => ImportResult::Err{
				error:anyhow!("Unexpected database reply when storing {}",path.as_ref().to_string_lossy()),
				filename
			},
		},
		Err(e) => {return ImportResult::Err{
			error:e.context(format!("when storing {}",path.as_ref().to_string_lossy())),
			filename
		};}
	}
}


pub(crate) fn import_glob<T>(pattern:T) -> Result<impl Stream<Item=Result<ImportResult,JoinError>>> where T:AsRef<str>
{
	let mut tasks=tokio::task::JoinSet::new();
	let mut files= glob(pattern.as_ref())
		.map_err(|e|anyhow!("Invalid globbing pattern {}:({})",pattern.as_ref(),e))?
		.filter_map(|f|
			if let Ok(f)=f{
				if f.is_file(){Some(f)} else {None}
			} else {None}
		);

	//pre-fill first 10 register tasks so there will always be some tasks that can do stuff
	for _ in 0..9{
		files.next().map(|nextfile|
			tasks.spawn(import_file(nextfile)));
	}
	// make a stream that polls tasks an feeds new ones
	let stream=futures::stream::poll_fn(move |c|{
		let p=tasks.poll_join_next(c);
		if p.is_ready() {//if a task finished
			// spawn a new one, if there are still files in the globber
			files.next().map(|nextfile|
				tasks.spawn(import_file(nextfile)));
		}
		p // then just send the Poll along for the stream to deal with
	});
	Ok(stream)
}

pub fn import_glob_as_text<T>(pattern:T) -> anyhow::Result<impl Stream<Item=Result<String,JoinError>>> where T:AsRef<str>
{
	Ok(import_glob(pattern)?
		.map_ok(|item|match item {
			ImportResult::Registered { filename } => format!("{filename} stored"),
			ImportResult::Existed { filename, .. } => format!("{filename} already existed"),
			ImportResult::Err { filename, error } => format!("Failed to register {filename}: {error}")
		})
	)
}
