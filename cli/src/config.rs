use std::path::PathBuf;
use config::{Config,ConfigError, File, FileFormat::Toml};
use std::sync::OnceLock;
use serde::Deserialize;

static CONFIG:OnceLock<Config> = OnceLock::new();

static CONFIG_STR:&str = r#"
study_tags = ["StudyDescription", "OperatorsName", "ManufacturerModelName"] #PatientID, StudyTime and StudyDate will always be there they are needed internally
series_tags = ["SequenceName", "SeriesDate", "SeriesTime", "ProtocolName"] #SeriesDescription and SeriesNumber will always be there they are needed internally
instance_tags = ["InstanceCreationDate", "InstanceCreationTime"] # InstanceNumber will always be there as its needed internally

upload_sizelimit_mb = 10

#use dicom tag names in "{}" to generate file names (obviously those should be unique)
#tag names can be followed by ":<" or ":>" and a number where resulting string will be cropped to the given size by
#removing caracters from the right or left respectively
filename_pattern = "{PatientID}/{StudyDate:>6}_{StudyTime:<6}/S{SeriesNumber}_{SeriesDescription}/Mr.{SOPInstanceUID}.ima"
storage_path = "/tmp/db/store" #will be used if filename_pattern does not result in an absolute path
"#;

pub fn init(config_file:Option<PathBuf>) -> Result<(),ConfigError>{
	let mut builder = Config::builder()
		.add_source(File::from_str(CONFIG_STR,Toml));
	if let Some(filename) = config_file {
		let filename = filename.to_str().expect("Failed to encode filename as UTF-8");
		builder=builder.add_source(File::new(filename,Toml));
	}
	CONFIG.set(builder.build()?).ok();
	let storage_path:PathBuf = get("storage_path")
		.expect(r#""storage_path" is missing in the config"#);
	if !storage_path.is_absolute(){
		return Err(ConfigError::Foreign(r#""storage_path" must be an absolute path"#.into()))
	}
	if !storage_path.exists(){
		return Err(ConfigError::Foreign(r#""storage_path" must exist"#.into()))
	}
	Ok(())
}

pub fn write(path:PathBuf) -> Result<(),ConfigError>
{
	std::fs::write(path,CONFIG_STR).map_err(|e|ConfigError::Foreign(Box::new(e)))
}

pub(crate) fn get<'de, T: Deserialize<'de>>(key: &str) -> Result<T,ConfigError>
{
	CONFIG.get()
		.expect("accessing uninitialized global config")
		.get(key)
}
