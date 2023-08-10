use std::path::PathBuf;
use surrealdb::sql::Thing;
use anyhow::{anyhow, bail, Result};
use tokio::fs::{remove_file,remove_dir};
use crate::db::{query_for_list, unregister};

pub async fn remove(id:Thing) -> Result<()>{
	let instances= match id.tb.as_str() {
		"studies" => query_for_list(id,"series.instances").await?,
		"series" => query_for_list(id,"instances").await?,
		"instances" => vec![id],
		_ => bail!("Invalid table name {} (available [\"studies\",\"series\",\"instances\"])",id.tb)
	};
	for instance in instances{
		remove_instance(instance).await?
	}
	todo!()
}

async fn remove_instance(id:Thing) -> Result<()>
{
	if let Some(removed)=unregister(id).await?{
		let file = removed.get("file")
			.ok_or(anyhow!("missing file data in deleted instance"))?;
		let owned = file.get("owned").map_or(false,|v|v.as_bool().unwrap());
		if owned {
			let path:PathBuf = file.get("path").unwrap().as_str().unwrap().into();
			remove_file(&path).await?;
			if let Err(e) = remove_dir(path.parent().unwrap()).await {
				dbg!(e);
				// if e.kind() == ErrorKind::DirectoryNotEmpty {
				// }
			}
		}
	} else {};
	todo!()
}
