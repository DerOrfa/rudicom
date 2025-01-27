use std::path::PathBuf;
use config::{Config,ConfigError, File, FileFormat::Toml};
use std::sync::OnceLock;
use serde::Deserialize;

static CONFIG:OnceLock<Config> = OnceLock::new();

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

	CONFIG.set(builder.build()?).ok();
	let storage_path:PathBuf = get("paths.storage_path")
		.expect(r#""storage_path" is missing in the config"#);
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

pub(crate) fn get<'de, T: Deserialize<'de>>(key: &str) -> Result<T,ConfigError>
{
	CONFIG.get()
		.expect("accessing uninitialized global config")
		.get(key)
}
