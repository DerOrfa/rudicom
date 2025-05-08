use crate::common::dcm::{synthesize_study,bulk_insert, UidSynthesizer};
use crate::common::init_db;
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
use glob::glob;
use itertools::Itertools;
use rand::random;
use rudicom::db::{lookup_uid, AggregateData, DB};
use rudicom::tools::remove::remove;
use tokio::task::JoinSet;

mod common;

async fn check_statistics(uid_gen: &UidSynthesizer, data:&Vec<Vec<FileDicomObject<InMemDicomObject>>>) -> Result<(), Box<dyn std::error::Error>>
{
	// check statistics
	let study_id = uid_gen.study(111);
	let study_entry = lookup_uid("studies",study_id).await?
		.expect("expected study entry");
	let instances_per_study= study_entry.get_instances_per().await?.count;
	assert_eq!(instances_per_study,data.iter().flatten().count(),"expected number of instances in study-statistics to match data");
	for i in 0..10{
		let ser_id = uid_gen.series(111,i);
		let series_entry = lookup_uid("series",ser_id).await?
			.expect("expected series entry");
		let instances_per_series= series_entry.get_instances_per().await?.count;
		assert_eq!(instances_per_series,data[i as usize].len(), "expected number of instances in statistics for series {i} to match data");
	};
	let store_path = rudicom::config::get().paths.storage_path.display();
	let files = glob(format!("{}/**/*",store_path).as_str())?
		.filter_map_ok(|p| if p.is_file() {Some(p)} else {None})
		.count();
	assert_eq!(files,data.iter().flatten().count(), "expected number of files in {store_path} to match number of instances");

	Ok(())
}

#[tokio::test]
async fn study() -> Result<(), Box<dyn std::error::Error>>
{
	init_db().await?.health().await?;
	let uid_gen = UidSynthesizer::default();
	let mut instances = synthesize_study(&uid_gen,111,10,100);
	for stored in bulk_insert(instances.iter().flatten()).await?
	{
		assert!(stored.is_none(), "unexpected return from first store");
	}
	// check for all objects to be there
	for obj in instances.iter().flatten()
	{
		let look = obj.element(tags::SOP_INSTANCE_UID)?.string()?;
		let found = lookup_uid("instances",look.to_string()).await?;
		assert!(found.is_some(), "stored instance {look} not found in DB");
		let file = found.unwrap().get_file()?;
		file.verify().await
			.map_err(|e| format!("failed to verify stored file for {look}: {e}"))?;
		// dicom read/write is not guaranteed to be symmetric
		// let stored= file.read().await
		// 	.map_err(|e| format!("failed to read stored file for {look}: {e}"))?;
		// assert_eq!(&stored,obj);
	}
	// check statistics after creation
	check_statistics(&uid_gen,&instances).await?;
	
	// remove 100 random entries and check statistics again
	let mut remove_set = JoinSet::new();
	for _i in 0..100{
		let r_ser=random::<u8>() % instances.len() as u8;
		let r_inst = random::<u8>() % instances[r_ser as usize].len() as u8;
		let to_be_removed = instances[r_ser as usize][r_inst as usize].element(tags::SOP_INSTANCE_UID)?
			.string()?.to_string();
		remove_set.spawn(async move {
			let to_be_removed = lookup_uid("instances",to_be_removed.to_string()).await.unwrap()
				.expect("instance to be removed not found in DB");
			remove(to_be_removed.id()).await
		});
		instances[r_ser as usize].remove(r_inst as usize);
	}
	remove_set.join_all().await.into_iter().collect::<Result<Vec<()>,_>>()
		.map_err(|e| format!("failed to remove instances: {e}"))?;

	check_statistics(&uid_gen,&instances).await?;

	// delete all
	let mut remove_set = JoinSet::new();
	for obj in instances.into_iter().flatten() {
		let to_be_removed = obj.element(tags::SOP_INSTANCE_UID)?.string()?.to_string();
		remove_set.spawn(async move {
			let to_be_removed = lookup_uid("instances",to_be_removed.to_string()).await.unwrap()
				.expect("instance to be removed not found in DB");
			remove(to_be_removed.id()).await
		});
	}
	remove_set.join_all().await.into_iter().collect::<Result<Vec<()>,_>>()
		.map_err(|e| format!("failed to remove remaining instances: {e}"))?;

	let store_path = rudicom::config::get().paths.storage_path.display();
	let files = glob(format!("{}/**/*",store_path).as_str())?.count();
	assert_eq!(files,0, "expected number of files to be 0 after removing all instances");


	let studies_v:Vec<AggregateData> = DB.select("instances_per_studies").await?;
	let instances = studies_v.iter()
		.map(|v|v.count)
		.reduce(|a,b|a+b).unwrap_or(0);
	let studies = studies_v.len();
	assert_eq!(instances,0,"{} instances found in {} studies where they should be none",instances,studies);

	Ok(())
}
