use std::path::PathBuf;
use dicom::dictionary_std::tags;
use glob::glob;
use crate::db;
use crate::dcm::{INSTACE_TAGS,SERIES_TAGS,STUDY_TAGS};

mod async_store;

pub async fn register_file(path:PathBuf) -> anyhow::Result<db::JsonValue>{
	let file = async_store::read_file(path.clone()).await?;

	let mut instance_meta = db::prepare_meta_for_db(&file,INSTACE_TAGS.clone(),"instances",tags::SOP_INSTANCE_UID)?;
	let series_meta = db::prepare_meta_for_db(&file, SERIES_TAGS.clone(), "series", tags::SERIES_INSTANCE_UID)?;
	let study_meta = db::prepare_meta_for_db(&file, STUDY_TAGS.clone(), "studies", tags::STUDY_INSTANCE_UID)?;

	let path = path.into_os_string().into_string().unwrap();
	instance_meta.insert(String::from("path"),path.into());

	Ok(db::register(instance_meta,series_meta,study_meta).await?)
}

pub async fn import_glob(pattern:&str){
	let mut tasks=tokio::task::JoinSet::new();
	for entry in glob(pattern).expect("Failed to read glob pattern") {
		match entry {
			Ok(path) => {
				tasks.spawn(async { (path.clone(), register_file(path).await)});
				if tasks.len() > 10 { process_file_task(&mut tasks).await;}
			},
			Err(e) => eprintln!("{e:?}")
		}
	}

	while process_file_task(&mut tasks).await {	}
}

async fn process_file_task<T:'static>(tasks: &mut tokio::task::JoinSet<(PathBuf,anyhow::Result<T>)>) -> bool{
	if let Some(result) = tasks.join_next().await{
		match result {
			Ok((path,res)) => {
				match res {
					Err(e) => {eprintln!("Error processing {}:{e}",path.to_str().unwrap())}
					_ => {}
				}
			}
			Err(e) => {eprintln!("{e}")}
		}
		true
	} else {false}
}
