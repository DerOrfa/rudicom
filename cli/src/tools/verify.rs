use surrealdb::sql::Thing;
use crate::db::File;
use crate::tools::{instances_for_entry, lookup_instance_file};
use crate::tools::Error;

pub async fn verify_entry(id:Thing) -> crate::tools::Result<Vec<File>>
{
    let mut jobs=tokio::task::JoinSet::new();
    for job in instances_for_entry(id).await?
        .into_iter().map(verify_instance)
    {
        jobs.spawn(job);
    }
    let mut ret=Vec::new();
    while let Some(result) = jobs.join_next().await {
        ret.push(result??);
    }
    Ok(ret)
}

async fn verify_instance(instance: Thing) -> crate::tools::Result<File>
{
    match lookup_instance_file(instance.id.to_raw().as_str()).await?
    {
        Some(file) => file.verify().await.and(Ok(file)),
        None => Err(Error::IdNotFound{id:instance})
    }
}
