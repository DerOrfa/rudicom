use std::path::PathBuf;
use once_cell::sync::Lazy;
use config::{Config, ConfigError, File, FileFormat::Toml};
use std::sync::RwLock;
use anyhow::bail;
use serde::Deserialize;

static CONFIG:Lazy<RwLock<Config>> = Lazy::new(||RwLock::new(Config::default()));
static CONFIG_STR:&str = r#"
instace_tags = ["InstanceCreationDate", "InstanceCreationTime", "InstanceNumber"]
series_tags = ["ProtocolName", "SequenceName", "SeriesDate", "SeriesTime", "SeriesDescription", "SeriesNumber"]
study_tags = ["StudyTime", "StudyDate", "StudyDescription", "OperatorsName", "ManufacturerModelName"]
"#;

pub fn init(config_file:Option<PathBuf>) -> anyhow::Result<()>{
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

pub fn get<'de, T: Deserialize<'de>>(key: &str) -> Result<T, ConfigError>{
	CONFIG.read().unwrap().get(key)
}
