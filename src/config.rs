use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;
use config::{Config,ConfigError, File, FileFormat::Toml};
use std::sync::OnceLock;
use dicom::core::DataDictionary;
use dicom::dictionary_std::StandardDataDictionary;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::dcm::AttributeSelector;

#[derive(Debug,Serialize,Deserialize)]
pub struct Limits{pub upload_sizelimit:byte_unit::Byte, pub max_files:u16}
#[derive(Debug,Serialize,Deserialize)]
pub struct Paths{pub filename_pattern:String,pub storage_path:PathBuf}

#[derive(Debug,Serialize,Deserialize)]
pub struct ConfigStruct
{
	pub instance_tags:HashMap<String,Vec<AttributeSelector>>,
	pub series_tags:HashMap<String,Vec<AttributeSelector>>,
	pub study_tags:HashMap<String,Vec<AttributeSelector>>,
	pub limits: Limits,
	pub paths: Paths
}

impl Serialize for AttributeSelector {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer
	{
		serializer.serialize_str(self.0.to_string().as_str())
	}
}
impl<'de> Deserialize<'de> for AttributeSelector {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de>
	{
		let dict = StandardDataDictionary::default();

		let parts=String::deserialize(deserializer)?;
		dict.parse_selector(parts.as_str())
			.map(AttributeSelector)
			.map_err(serde::de::Error::custom)
	}
}



static CONFIG:OnceLock<ConfigStruct> = OnceLock::new();

static CONFIG_STR:&str = include_str!("config.toml");

pub fn init(config_file:Option<PathBuf>) -> Result<(),ConfigError>{
	let mut builder = Config::builder()
		.add_source(File::from_str(CONFIG_STR,Toml));
	if let Some(filename) = config_file {
		let filename = filename.canonicalize().map_err(|e|ConfigError::Foreign(Box::new(e)))?;
		let filename = filename.to_str()
				.ok_or(ConfigError::Foreign(format!("Failed to encode filename {} as UTF-8",filename.to_string_lossy()).into()))?;
		tracing::info!("loading config from {filename}");
		builder=builder.add_source(File::new(filename,Toml));
	} else {
		tracing::warn!(r#"no config file given loading defaults (use "write-config" subcommand to write it to a file)"#);
	}

	CONFIG.set(builder.build().unwrap().try_deserialize()?).expect("Failed to set config");

	let storage_path = &get().paths.storage_path;
	if !storage_path.is_absolute(){
		return Err(ConfigError::Foreign(format!(r#""{}" (the storage path) must be an absolute path"#,storage_path.to_string_lossy()).into()))
	}
	if !storage_path.exists(){
		return Err(ConfigError::Foreign(format!(r#""{}" (the storage path) must exist"#,storage_path.to_string_lossy()).into()))
	}
	Ok(())
}

pub fn write(path:PathBuf) -> Result<(),ConfigError>
{
	std::fs::write(path,CONFIG_STR).map_err(|e|ConfigError::Foreign(Box::new(e)))
}

pub fn get() -> &'static ConfigStruct { CONFIG.get().unwrap() }
