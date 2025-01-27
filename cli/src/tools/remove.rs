use crate::db;
use crate::tools::entries_for_record;
use crate::tools::Result;
use std::path::{Path, PathBuf};
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
	while let Some(result) = jobs.join_next().await.transpose()? {
		result?;
	}
	Ok(())
}

async fn remove_instance(id:db::RecordId) -> Result<Option<db::Entry>>
{
	let res = DB.delete(id).await?;
	if res.into_inner_ref().is_none_or_null(){
		return Ok(None)
	}
	let removed= db::Entry::try_from(res)?;
	let file = removed.get_file()?;
	if file.owned {
		let mut path = file.get_path();
		remove_file(&path).await?;
		if path.pop(){// if there is a parent path, try to delete it as far as possible
			let storage_path:PathBuf = crate::config::get("paths.storage_path").expect(r#"Failed to get "storage_path""#);
			remove_path(path,storage_path.as_path()).await?;
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
			if e.raw_os_error() == Some(39) //dir is not empty (that's fine, we'll just stop deleting)
			{
				return Ok(());
			}
			return Err(e);
		}
		if !path.pop() || path == stop_path
		{
			return Ok(());
		}
	}
}
