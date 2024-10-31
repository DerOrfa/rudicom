use crate::db::Entry;
use crate::tools::{Error, Result};
use std::cmp::min;

/// verify all instances below that entry. Returns list of failed instances.
pub async fn verify_entry(entry:Entry) -> Result<Vec<Error>>
{
    let mut jobs=tokio::task::JoinSet::new();
    let max_files = crate::config::get::<usize>("max_files").unwrap_or(32);
    let mut files = entry.get_files().await?;

    // pre-fill jobs
    for file in files.drain(..min(max_files,files.len()))
    {
        jobs.spawn(async { file.verify().await.map(|_|file) });
    }

    let mut ret=Vec::new();
    while let Some(result) = jobs.join_next().await.transpose()? {
        //for each completed verify add a new job
        if let Some(file) = files.pop(){ // if there are files left
            jobs.spawn(async { file.verify().await.map(|_|file) });
        }
        if let Err(err) = result{ret.push(err)}
    }
    Ok(ret)
}
