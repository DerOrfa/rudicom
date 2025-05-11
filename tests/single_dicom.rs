use crate::common::dcm;
use crate::common::init_db;
use dicom::dictionary_std::tags;
use rudicom::db::lookup;
use rudicom::db::RegisterResult;
use rudicom::tools::store::store;
use crate::common::dcm::cleanup;

mod common;

#[tokio::test]
async fn single_dicom() -> Result<(), Box<dyn std::error::Error>>
{
	init_db().await?.health().await?;
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	if let RegisterResult::Stored(_) = store(obj.clone()).await? {} 
	else { panic!("First store should return stored."); }
	if let RegisterResult::AlreadyStored(stored) = store(obj.clone()).await? {
		let stored = lookup(&stored).await?.expect("existing object should be found");
		let path = stored.get_file()?.get_path();
		let red = dicom::object::open_file(&path)?;
		assert_eq!(stored.id().str_key(), red.element(tags::SOP_INSTANCE_UID)?.string()?);

		rudicom::tools::remove::remove(stored.id()).await?;
		assert!(!path.exists(), "File should be gone after remove.");
	} 
	else { panic!("Second store should return already stored."); }

	cleanup().await.map_err(|e| e.into())
}
