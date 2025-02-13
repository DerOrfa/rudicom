use crate::db;
use crate::tools::{entries_for_record, Context};
use crate::tools::Result;
use std::path::{Path, PathBuf};
use surrealdb::err::Error::QueryNotExecutedDetail;
use tokio::fs::{remove_dir, remove_file};
use crate::db::DB;

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
	let mut res = Ok(Default::default());
	loop {
		res = DB.delete(id.clone()).await;
		match &res {
			Err(surrealdb::Error::Db(QueryNotExecutedDetail{message})) => {
				if message != "Failed to commit transaction due to a read or write conflict. This transaction can be retried" {break}
			}
			_ => {break},
		}
	}
	let res = res?; 
	if res.into_inner_ref().is_none_or_null(){
		return Ok(None)
	}
	let removed= db::Entry::try_from(res)?;
	let file = removed.get_file()?;
	if file.owned {
		let mut path = file.get_path();
		remove_file(&path).await.context(format!("deleting {}",path.to_string_lossy()))?;
		if path.pop(){// if there is a parent path, try to delete it as far as possible
			let ctx = format!("deleting {}",path.to_string_lossy());
			remove_path(path,&crate::config::get().paths.storage_path)
				.await.context(ctx)?;
		}
	}
	Ok(Some(removed))
}

/// removes given directory and all parents until path is empty or stop_path is reached
async fn remove_path(mut path:PathBuf, stop_path:&Path) -> std::io::Result<()>
{
	loop {
		if let Err(e) = remove_dir(path.as_path()).await
		{
			match e.kind() { 
				std::io::ErrorKind::NotFound => return Ok(()), //dir is not empty (that's fine, we'll just stop deleting)
				std::io::ErrorKind::DirectoryNotEmpty => return Ok(()), //dir is gone already (that's fine, some other thread deleted it)
				_ => return Err(e),
			}
		}
		if !path.pop() || path == stop_path
		{
			return Ok(());
		}
	}
}
