use std::path::PathBuf;
use once_cell::sync::Lazy;
use config::{Config, File, FileFormat::Toml};
use std::sync::RwLock;
use anyhow::bail;
use serde::Deserialize;
use crate::Result;

static CONFIG:Lazy<RwLock<Config>> = Lazy::new(||RwLock::new(Config::default()));
static CONFIG_STR:&str = r#"
instace_tags = ["InstanceCreationDate", "InstanceCreationTime", "InstanceNumber"]
series_tags = ["ProtocolName", "SequenceName", "SeriesDate", "SeriesTime", "SeriesDescription", "SeriesNumber"]
study_tags = ["PatientID", "StudyTime", "StudyDate", "StudyDescription", "OperatorsName", "ManufacturerModelName"]

filename_pattern = "{PatientID}/{StudyDate}_{StudyTime}/S{SeriesNumber}_{SeriesDescription}/Mr.{SOPInstanceUID}.ima"
storage_path = "/tmp/db/store" #will be use if filename_pattern does not result in an absolute path
"#;

pub fn init(config_file:Option<PathBuf>) -> Result<()>{
	let mut builder = Config::builder()
		.add_source(File::from_str(CONFIG_STR,Toml));
	if config_file.is_some() {
		let Some(filename) = config_file.as_ref().unwrap().to_str()
			else {bail!("Failed to encode filename as UTF-8")};
		builder=builder.add_source(File::new(filename,Toml));
	}
	CONFIG.write().unwrap().clone_from(&builder.build()?);
	Ok(())
}

pub fn write(path:PathBuf) -> Result<()>
{
	std::fs::write(path,CONFIG_STR).map_err(|e|e.into())
}

pub(crate) fn get<'de, T: Deserialize<'de>>(key: &str) -> Result<T>
{
	CONFIG.read().unwrap().get(key).map_err(|e|e.into())
}
