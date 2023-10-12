use std::path::{Path, PathBuf};
use surrealdb::sql::Thing;
use anyhow::{bail, Result};
use tokio::fs::{remove_file,remove_dir};
use crate::db;

pub async fn remove(id:Thing) -> Result<()>{
	let instances= match id.tb.as_str() {
		"studies" => db::query_for_list(id,"series.instances").await?,
		"series" => db::query_for_list(id,"instances").await?,
		"instances" => vec![id],
		_ => bail!("Invalid table name {} (available [\"studies\",\"series\",\"instances\"])",id.tb)
	};
	for instance in instances{
		remove_instance(instance).await?;
	}
	Ok(())
}

async fn remove_instance(id:Thing) -> Result<Option<db::Entry>>
{
	let res= db::unregister(id).await?;
	if res.is_none(){
		Ok(None)
	} else {
		let removed = db::Entry::try_from(res)?;
		let file = removed.get_file()?;
		if file.owned {
			let storage_path = crate::config::get::<PathBuf>("storage_path").expect(r#""storage_path" missing or invalid in config"#);
			let path = storage_path.join(file.path.as_str());
			remove_file(&path).await?;
			remove_path(path.parent().unwrap().to_path_buf(),storage_path.as_path()).await?;
		}
		Ok(Some(removed))
	}
}

async fn remove_path(mut path:PathBuf, storage_path:&Path) -> std::io::Result<()>
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
		if !path.pop() || path == storage_path
		{
			return Ok(());
		}
	}
}
