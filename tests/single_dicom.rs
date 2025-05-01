use crate::common::dcm;
use crate::common::init_db;
use dicom::dictionary_std::tags;
use rudicom::tools::store::store;
use crate::common::dcm::cleanup;

mod common;

#[tokio::test]
async fn single_dicom() -> Result<(), Box<dyn std::error::Error>>
{
	init_db().await?.health().await?;
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	let res = store(obj.clone()).await?;
	assert!(res.is_none(), "First store should return None.");
	let stored = store(obj.clone()).await?.expect("Second store should return first store.");

	let path = stored.get_file()?.get_path();
	let red = dicom::object::open_file(&path)?;
	assert_eq!(stored.id().str_key(), red.element(tags::SOP_INSTANCE_UID)?.string()?);
	
	rudicom::tools::remove::remove(stored.id()).await?;
	assert!(!path.exists(), "File should be gone after remove.");
	cleanup().await.map_err(|e| e.into())
}
