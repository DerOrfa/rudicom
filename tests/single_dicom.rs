use crate::common::dcm;
use crate::common::init_db;
use dicom::dictionary_std::tags;
use tracing::debug;
use rudicom::db::{lookup, ArcSession, DB};
use rudicom::db::RegisterResult;
use rudicom::tools::store::store;
use crate::common::dcm::cleanup;

mod common;

#[tokio::test]
async fn single_dicom() -> Result<(), Box<dyn std::error::Error>>
{
	// tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).init();
	init_db().await?.health().await?;
	let session = ArcSession::new(&DB);
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	if let RegisterResult::Stored(_) = store(obj.clone(), session.clone()).await? {}
	else { panic!("First store should return stored."); }
	debug!("inserted object");
	if let RegisterResult::AlreadyStored(stored) = store(obj.clone(), session).await? {
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
