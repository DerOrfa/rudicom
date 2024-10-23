use crate::db::{File, RecordId};
use crate::tools::entries_for_record;

pub async fn verify_entry(id:&RecordId) -> crate::tools::Result<Vec<File>>
{
    let mut jobs=tokio::task::JoinSet::new();
    let files:crate::tools::Result<Vec<_>> = entries_for_record(&id,"instances").await?.into_iter()
        .map(|e|e.get_file()).collect(); 
    for file in files? 
    {
        jobs.spawn(async { file.verify().await.map(|_|file) });
    }
    let mut ret=Vec::new();
    while let Some(result) = jobs.join_next().await {
        let file = result??; 
        ret.push(file);
    }
    Ok(ret)
}
