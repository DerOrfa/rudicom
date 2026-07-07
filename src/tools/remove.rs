use crate::db;
use crate::tools::entries_for_record;
use crate::tools::Result;
use std::path::{Path, PathBuf};
use crate::db::{if_retry, DB};
use surrealdb::types as db_types;
use tokio::fs::remove_dir;

pub async fn remove(id:&db::RecordId) -> Result<()>
{
	let mut jobs=tokio::task::JoinSet::new();
	for job in entries_for_record(id,"instances").await?
		.into_iter().map(|e|remove_instance(e.id().clone()))
	{
		jobs.spawn(job);
	}
	let res:Result<Vec<_>> = jobs.join_all().await.into_iter().collect();
	res.map(|_|())
}

async fn remove_instance(id:db::RecordId) -> Result<Option<db::Entry>>
{
	let mut res;
	let mut retry = 0;
	loop {
		res = DB.delete::<Option<db_types::Value>>(id.0.clone()).await;
		match &res {
			Err(e) => if if_retry(e,&mut retry).await?{continue},
			_ => {break},
		}
	}
	let res = res?.unwrap();
	if res.is_nullish(){
		return Ok(None)
	}
	let removed= db::Entry::try_from(res)?;
	removed.get_file()?.remove().await?;
	Ok(Some(removed))
}

/// removes given directory and all parents until path is empty or stop_path is reached
pub async fn remove_path(mut path:PathBuf, stop_path:&Path) -> std::io::Result<()>
{
	loop {
		if let Err(e) = remove_dir(path.as_path()).await
		{
			return match e.kind() {
				std::io::ErrorKind::NotFound => Ok(()), //dir is gone already (that's fine, some other thread deleted it)
				std::io::ErrorKind::DirectoryNotEmpty => Ok(()), //dir is not empty (that's fine, we'll just stop deleting)
				_ => {
					// if it is actually gone, just ignore the error. Because:
					// "if a path does not exist, its removal may fail for a number of reasons, such as insufficient permissions"
					if path.exists() { Err(e) } else { Ok(()) }
				}
			}
		}
		if !path.pop() || path == stop_path
		{
			return Ok(());
		}
	}
}
