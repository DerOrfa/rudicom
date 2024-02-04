use std::path::PathBuf;
use config::{Config, File, FileFormat::Toml};
use std::sync::OnceLock;
use serde::Deserialize;
use crate::tools::{Result,Context};

static CONFIG:OnceLock<Config> = OnceLock::new();

static CONFIG_STR:&str = r#"
study_tags = ["StudyDescription", "OperatorsName", "ManufacturerModelName"] #PatientID, StudyTime and StudyDate will always be there they are needed internally
series_tags = ["SequenceName", "SeriesDate", "SeriesTime", "ProtocolName"] #SeriesDescription and SeriesNumber will always be there they are needed internally
instance_tags = ["InstanceCreationDate", "InstanceCreationTime"] # InstanceNumber will always be there as its needed internally

upload_sizelimit_mb = 10

filename_pattern = "{PatientID}/{StudyDate:>6}_{StudyTime:<6}/S{SeriesNumber}_{SeriesDescription}/Mr.{SOPInstanceUID}.ima"
storage_path = "/tmp/db/store" #will be used if filename_pattern does not result in an absolute path
"#;

pub fn init(config_file:Option<PathBuf>) -> Result<()>{
	let mut builder = Config::builder()
		.add_source(File::from_str(CONFIG_STR,Toml));
	if let Some(filename) = config_file {
		let filename = filename.to_str().expect("Failed to encode filename as UTF-8");
		builder=builder.add_source(File::new(filename,Toml));
	}
	CONFIG.set(builder.build()?).expect("failed initializing config");
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
		.get(key).context(format!("looking for {key} in configuration"))
}
