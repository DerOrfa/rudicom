use std::collections::HashMap;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use config::{Config,ConfigError, File, FileFormat::Toml};
use std::sync::OnceLock;
use dicom::core::DataDictionary;
use dicom::dictionary_std::StandardDataDictionary;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::dcm::AttributeSelector;

#[derive(Debug,Serialize,Deserialize)]
pub struct Limits{pub upload_sizelimit:byte_unit::Byte, pub max_files:u16, pub db_capacity:usize}
#[derive(Debug,Serialize,Deserialize)]
pub struct Paths{pub filename_pattern:String,pub storage_path:PathBuf}
#[derive(Debug,Serialize,Deserialize)]
pub struct DimseCfg{
	pub aet:String,
	pub address:String,
	pub peers:HashMap<String,SocketAddr>,
}

#[derive(Debug,Serialize,Deserialize)]
pub struct ConfigStruct
{
	pub instance_tags:HashMap<String,Vec<AttributeSelector>>,
	pub series_tags:HashMap<String,Vec<AttributeSelector>>,
	pub study_tags:HashMap<String,Vec<AttributeSelector>>,
	pub limits: Limits,
	pub paths: Paths,
	pub dimse: DimseCfg

}

impl Serialize for AttributeSelector {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer
	{
		let v = match self {
			AttributeSelector::Core(c) => c.to_string(),
			AttributeSelector::CSA { base, element } => format!("{}#CSA.{}", base, element),
		};
		serializer.serialize_str(v.as_str())
	}
}
impl<'de> Deserialize<'de> for AttributeSelector {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de>
	{
		let dict = StandardDataDictionary::default();

		let parts=String::deserialize(deserializer)?;
		if let Some(p) = parts.find("#CSA"){
			let base = &parts[..p];
			let base = dict.parse_selector(base).map_err(serde::de::Error::custom)?;
			Ok(AttributeSelector::CSA {base,element:parts[p+5..].to_string()})
		} else {
			dict.parse_selector(parts.as_str())
				.map(AttributeSelector::Core)
				.map_err(serde::de::Error::custom)
		}
	}
}



static CONFIG:OnceLock<ConfigStruct> = OnceLock::new();

static CONFIG_STR:&str = include_str!("config.toml");

pub fn init(config_file:Option<PathBuf>) -> Result<(),ConfigError>{
	let mut builder = Config::builder()
		.set_default(
			"paths.storage_path",
			std::env::temp_dir().join("db_store").to_string_lossy().into_owned()
		)?
		.add_source(File::from_str(CONFIG_STR,Toml));
	if let Some(filename) = config_file {
		let filename = filename.canonicalize().map_err(|e|ConfigError::Foreign(Box::new(e)))?;
		tracing::info!("loading config from file {}",filename.display());
		builder=builder.add_source(File::from(filename).format(Toml));
	} else {
		tracing::warn!(r#"no config file given loading defaults (use "write-config" subcommand to write it to a file)"#);
	}

	CONFIG.set(builder.build().unwrap().try_deserialize()?).expect("Failed to set config");
	Ok(())
}

pub fn write(path:&Path) -> Result<(),ConfigError>
{
	std::fs::write(path,CONFIG_STR).map_err(|e|ConfigError::Foreign(Box::new(e)))
}

pub fn get() -> &'static ConfigStruct { CONFIG.get().unwrap() }
