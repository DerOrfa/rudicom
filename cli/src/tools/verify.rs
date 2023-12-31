use anyhow::{anyhow, Context};
use surrealdb::sql::Thing;
use crate::tools::{instances_for_entry, lookup_instance_file};
use crate::tools::store::AsyncMd5;

pub async fn verify_entry(id:Thing) -> anyhow::Result<()>
{
    let mut jobs=tokio::task::JoinSet::new();
    for job in instances_for_entry(id).await?
        .into_iter().map(verify_instance)
    {
        jobs.spawn(job);
    }
    while let Some(result) = jobs.join_next().await {
        result??;
    }
    Ok(())
}

async fn verify_instance(instance: Thing) -> anyhow::Result<String>
{
    if let Some(file)=lookup_instance_file(instance.id.to_raw().as_str()).await?
    {
        let md5_stored = file.md5.as_str();
        let mut md5_compute = AsyncMd5::new();
        let filename = file.get_path();
        let mut file = tokio::fs::File::open(&filename).await.context(format!("opening {}",filename.to_string_lossy()))?;
        tokio::io::copy(&mut file,&mut md5_compute).await?;
        let md5_computed = md5_compute.compute();
        if md5_computed == md5_stored {Ok(md5_computed)}
        else {Err(anyhow!("Checksum computed checksum {md5_computed} does not fit the stored checksum {md5_stored}"))}
    }
    else { Err(anyhow!("{instance} was not found")) }
}
