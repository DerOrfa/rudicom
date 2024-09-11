use crate::db;
use crate::tools::instances_for_entry;
use crate::tools::Result;
use std::path::{Path, PathBuf};
use surrealdb::RecordIdKey;
use tokio::fs::{remove_dir, remove_file};
use crate::db::RecordId;

pub async fn remove(id:db::RecordId) -> Result<()>
{
	let mut jobs=tokio::task::JoinSet::new();
	for job in instances_for_entry(id).await?
		.into_iter().map(remove_instance)
	{
		jobs.spawn(job);
	}
	while let Some(result) = jobs.join_next().await.transpose()? {
		result?;
	}
	Ok(())
}

async fn remove_instance<I>(id:I) -> Result<Option<db::Entry>>  where RecordIdKey: From<I>
{
	let res= db::unregister(RecordId::instance(id)).await?;
	if res.is_none() {
		Ok(None)
	} else {
		let removed = db::Entry::try_from(res)?;
		let file = removed.get_file()?;
		if file.owned {
			let mut path = file.get_path();
			remove_file(&path).await?;
			if path.pop(){// if there is a parent path, try to delete it as far as possible
				let storage_path:PathBuf = crate::config::get("storage_path").expect(r#"Failed to get "storage_path""#);
				remove_path(path,storage_path.as_path()).await?;
			}
		}
		Ok(Some(removed))
	}
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
