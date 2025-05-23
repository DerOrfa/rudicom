use crate::common::dcm::{bulk_insert, cleanup, synthesize_series, UidSynthesizer};
use crate::common::init_db;
use dicom::core::{DataElement, VR};
use dicom::dictionary_std::tags;
use rudicom::db::{lookup_uid, RegisterResult};
use rudicom::tools::Error;

mod common;

#[tokio::test]
async fn invalid_insert() -> Result<(), Box<dyn std::error::Error>>
{
	init_db().await?.health().await?;
	
	// set up some data register half of it
	let uid_gen= UidSynthesizer::default();
	let mut instances = synthesize_series(&uid_gen,111,10,100); 
	let (ins1, ins2) = instances.split_at_mut(50);
	for inserted in bulk_insert(ins1.iter()).await?{
		if let RegisterResult::Stored(_) = inserted {} else { panic!("unexpected return from initial store:{:?}",inserted); }
	}
	
	// mess around with the other half
	let str_tags = [
		(tags::STUDY_DATE, VR::DA, "20250102"),
		(tags::STUDY_TIME, VR::TM, "100000.000000"),
		(tags::STUDY_ID, VR::SH, "John_Mess"),
		(tags::PATIENT_NAME, VR::PN,"Mess^John",),
		(tags::PATIENT_ID, VR::LO,"John Mess",),
	].map(|(id,vr,val)|DataElement::new(id,vr,val)).into_iter();

	for mess in ins2.iter_mut(){
		mess.put_element(DataElement::new(tags::SERIES_NUMBER,VR::IS,12.to_string()));
		for tag in str_tags.clone(){
			mess.put_element(tag);
		}
	}
	
	let messy_insert = bulk_insert(ins2.iter()).await;
	let study_id = uid_gen.study(111);
	let study_entry = lookup_uid("studies",study_id).await?
		.expect("expected study entry");
	let instances_per_study= study_entry.get_instances_per().await?.count;
	assert_eq!(instances_per_study,ins1.len(),"Only {} instanced should have been inserted, but {} are there",ins1.len(),instances_per_study);

	match messy_insert{
		Err(Error::DataConflict(_)) => {}
		_ => panic!("Inserting conflicting data should result in DataConflict-Error, but result was {messy_insert:?}.")	
	}
	cleanup().await.map_err(|e| e.into())
}
