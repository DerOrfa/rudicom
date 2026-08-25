use std::io::ErrorKind;
use std::time::Duration;
use crate::common::{dcm, init_config};
use dicom::object::from_reader;
use tempfile::NamedTempFile;
use tokio::time::sleep;
use tracing_subscriber::filter::LevelFilter;
use rudicom::storage::Image;
use rudicom::tools::complete_filepath;
use rudicom::tools::Error::IoError;
use crate::common::dcm::diff;

static LOG_LEVEL: LevelFilter = LevelFilter::INFO;

mod common;
#[tokio::test]
async fn from_obj() -> Result<(), Box<dyn std::error::Error>>
{
	init_config().unwrap();
	tracing_subscriber::fmt().with_max_level(LOG_LEVEL).try_init().ok();

	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	let saved:Image = Image::from_obj(obj.clone()).into_saved().await.unwrap();
	
	// should be saved in storage
	let path = complete_filepath(saved.get_path());
	assert_eq!(diff(from_reader(std::fs::File::open(&path).unwrap()).unwrap().into_inner(), obj.clone().into_inner()), vec![]);
	drop(saved);
	// the drop creates a task to remove the file. Give it a chance to do that
	sleep(Duration::from_millis(100)).await;
	// and it should be gone
	assert!(!std::fs::exists(&path).unwrap());

	// create again
	let mut saved:Image = Image::from_obj(obj.clone()).into_saved().await.unwrap();
	saved.commit();
	sleep(Duration::from_millis(100)).await;
	// and it should *not* be gone
	assert!(std::fs::exists(&path).unwrap());

	// creating the same file should fail
	if let Err(IoError(e)) = Image::<rudicom::storage::file::StandardFile>::from_obj(obj.clone()).into_saved().await{
		assert_eq!(e.kind(), ErrorKind::AlreadyExists);
	} else { assert!(false); }

	// cleanup
	std::fs::remove_file(&path).unwrap();
	Ok(())
}

#[tokio::test]
async fn from_existing() -> Result<(), Box<dyn std::error::Error>>
{
	init_config().unwrap();
	tracing_subscriber::fmt().with_max_level(LOG_LEVEL).try_init().ok();

	// create an existing dicom file
	let tmp = NamedTempFile::with_prefix("from_existing_").unwrap();
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	obj.write_all(&tmp).unwrap();

	let existing:Image = Image::from_existing(tmp.path()).await.unwrap().into_saved().await.unwrap();
	// should be there
	let path = complete_filepath(existing.get_path());
	assert_eq!(path, tmp.path());
	assert!(std::fs::exists(&path).unwrap());

	drop(existing);
	sleep(Duration::from_millis(100)).await;
	// dropping this should not be doing anything
	assert!(std::fs::exists(&path).unwrap());
	Ok(())
}

#[tokio::test]
async fn move_existing() -> Result<(), Box<dyn std::error::Error>>
{
	init_config().unwrap();
	tracing_subscriber::fmt().with_max_level(LOG_LEVEL).try_init().ok();

	// create an existing dicom file
	let tmp = NamedTempFile::with_prefix("move_existing_").unwrap();
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	obj.write_all(&tmp).unwrap();

	let moved:Image = Image::move_existing(tmp.path()).await.unwrap().into_saved().await.unwrap();
	// should be there
	let path = complete_filepath(moved.get_path());
	assert!(std::fs::exists(&path).unwrap());
	assert_ne!(path, tmp.path()); // but not the same path as origin

	drop(moved);
	sleep(Duration::from_millis(100)).await;
	// should be gone
	assert!(!std::fs::exists(&path).unwrap());
	// but origin still be there
	assert!(std::fs::exists(tmp.path()).unwrap());

	// commit
	let mut moved:Image = Image::move_existing(tmp.path()).await.unwrap().into_saved().await.unwrap();
	moved.commit();
	// origin should be gone
	assert!(!std::fs::exists(tmp.path()).unwrap());
	// but ours should still be here
	assert!(std::fs::exists(&path).unwrap());

	// move existing, but it's the same file
	let moved:Image = Image::move_existing(&path).await.unwrap().into_saved().await.unwrap();
	drop(moved);
	sleep(Duration::from_millis(100)).await;
	// dropping the same should not remove it
	assert!(std::fs::exists(&path).unwrap());

	std::fs::remove_file(path).unwrap();

	Ok(())
}

#[tokio::test]
async fn copy_existing() -> Result<(), Box<dyn std::error::Error>>
{
	init_config().unwrap();
	tracing_subscriber::fmt().with_max_level(LOG_LEVEL).try_init().ok();

	// create an existing dicom file
	let tmp = NamedTempFile::with_prefix("copy_existing_").unwrap();
	let obj = dcm::synthesize_dicom_obj(&dcm::UidSynthesizer::default(), 1, 1, 1);
	obj.write_all(&tmp).unwrap();

	let moved:Image = Image::copy_existing(tmp.path()).await.unwrap().into_saved().await.unwrap();
	// should be there
	let path = complete_filepath(moved.get_path());
	assert!(std::fs::exists(&path).unwrap());
	assert_ne!(path, tmp.path()); // but not the same path as origin

	drop(moved);
	sleep(Duration::from_millis(100)).await;
	// should be gone
	assert!(!std::fs::exists(&path).unwrap());
	// but origin still be there
	assert!(std::fs::exists(tmp.path()).unwrap());

	// commit
	let mut moved:Image = Image::copy_existing(tmp.path()).await.unwrap().into_saved().await.unwrap();
	moved.commit();
	// origin should still be there
	assert!(std::fs::exists(tmp.path()).unwrap());
	// but ours as well
	assert!(std::fs::exists(&path).unwrap());

	// copy existing, onto the same file should fail
	if let Err(IoError(e)) = Image::<rudicom::storage::file::StandardFile>::copy_existing(&path).await.unwrap().into_saved().await{
		assert_eq!(e.kind(), ErrorKind::AlreadyExists);
	} else { assert!(false); }

	std::fs::remove_file(path).unwrap();

	Ok(())
}