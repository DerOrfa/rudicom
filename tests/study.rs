use dicom::dictionary_std::tags;
use crate::common::{dcm, init_db};
use crate::common::dcm::synthesize_study;
use tokio::task::JoinSet;
use rudicom::tools::store::store;
use rudicom::db::{lookup_uid, DB};

mod common;

#[tokio::test]
async fn study() -> Result<(), Box<dyn std::error::Error>>
{
	init_db().await?.health().await?;
	let uid_gen = dcm::UidSynthesizer::default();
	let instances = synthesize_study(&uid_gen,111,10,100);
	let mut set = JoinSet::new();
	for obj in instances.iter().flatten()
	{
		set.spawn(store(obj.clone()));
	}
	for stored in set.join_all().await.into_iter().collect::<Result<Vec<_>,_>>()?
	{
		assert!(stored.is_none(), "unexpected return from first store");
	}
	for obj in instances.iter().flatten()
	{
		let look = obj.element(tags::SOP_INSTANCE_UID)?.string()?;
		let found = lookup_uid("instances",look.to_string()).await?;
		assert!(found.is_some(), "stored instance {look} not found in DB");
	}
	Ok(())
}
