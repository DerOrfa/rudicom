use std::path::PathBuf;
use config::{Config, File, FileFormat::Toml};
use std::sync::OnceLock;
use anyhow::bail;
use serde::Deserialize;
use crate::Result;

static CONFIG:OnceLock<Config> = OnceLock::new();

static CONFIG_STR:&str = r#"
study_tags = ["StudyDescription", "OperatorsName", "ManufacturerModelName"] #PatientID, StudyTime and StudyDate will always be there they are needed internally
series_tags = ["SequenceName", "SeriesDate", "SeriesTime", "ProtocolName"] #SeriesDescription and SeriesNumber will always be there they are needed internally
instance_tags = ["InstanceCreationDate", "InstanceCreationTime"] # InstanceNumber will always be there as its needed internally

upload_sizelimit_mb = 10

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
	CONFIG.set(builder.build()?).expect("Failed to set global config");
	Ok(())
}

pub fn write(path:PathBuf) -> Result<()>
{
	std::fs::write(path,CONFIG_STR).map_err(|e|e.into())
}

pub(crate) fn get<'de, T: Deserialize<'de>>(key: &str) -> Result<T>
{
	CONFIG.get()
		.expect("accessing uninitialized global config")
		.get(key).map_err(|e|e.into())
}
