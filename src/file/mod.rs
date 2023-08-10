use std::collections::BTreeMap;
use std::path::PathBuf;
use anyhow::anyhow;
use glob::glob;
use md5::Context;
use crate::{db, register_instance};

pub mod async_store;

pub async fn register_file(path:PathBuf) -> anyhow::Result<db::JsonValue>{
	let mut md5=Context::new();
	let file = async_store::read_file(path.clone(),Some(&mut md5)).await?;

	let path = path.to_str().ok_or(anyhow!("Failed to encode filename in UTF-8"))?;

	let fileinfo:BTreeMap<String,db::DbVal>= BTreeMap::from([
		("path".into(),path.into()),
		("owned".into(),false.into()),
		("md5".into(),format!("{:x}", md5.compute()).into())
	]);
	register_instance(&file,vec![("file".into(),fileinfo.into())],None).await
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
