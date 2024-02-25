use std::path::PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap::builder::PossibleValue;
use clap::ValueHint::{Hostname,FilePath};
use tracing::Level;

#[cfg(feature = "embedded")]
use clap::ValueHint::DirPath;

#[derive(Clone)]
pub(super) struct LogLevel(Level);

impl ValueEnum for LogLevel
{
	fn value_variants<'a>() -> &'a [Self] 
	{
		&[
			LogLevel(Level::TRACE),
			LogLevel(Level::DEBUG),
			LogLevel(Level::INFO),
			LogLevel(Level::WARN),
			LogLevel(Level::ERROR)
		]
	}

	fn to_possible_value(&self) -> Option<PossibleValue> {
		Some(PossibleValue::new(self.0.as_str()))
	}
}

#[derive(Args,Debug)]
#[group(required = true, multiple = false)]
pub(super) struct Endpoint{
	/// hostname of the database
	#[arg(long, value_hint = Hostname)]
	pub(super) database: Option<String>,
	/// filename for the local database
	#[cfg(feature = "embedded")]
	#[arg(long, value_hint = DirPath)]
	pub(super) file:Option<PathBuf>
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(super) struct Cli {
	#[command(subcommand)]
	pub(super) command: Commands,
	/// config file
	#[arg(long, value_hint = FilePath)]
	pub(super) config: Option<PathBuf>,
	#[command(flatten)]
	pub(super) endpoint: Endpoint,
	/// logging level
	#[arg(long, default_value="warning")]
	pub(super) log_level:LogLevel
}

#[derive(Subcommand)]
pub(crate) enum Commands {
	/// writing the default config out into the given file
	WriteConfig {
		file:PathBuf
	},
	/// run the server
	Server {
		/// ip and port to listen on
		#[arg(default_value = "127.0.0.1:3000")]
		address: String,
	},
	/// import (big chunks of) data from the filesystem
	Import {
		/// report on already existing files
		#[arg(short,long,default_value_t=false)]
		existing:bool,
		/// report on imported files
		#[arg(short,long,default_value_t=false)]
		imported:bool,
		/// file or globbing to import
		pattern: String,
	},
}


pub(super) fn parse() -> Cli
{
	let ret=Cli::parse();

	tracing_subscriber::fmt().with_max_level(ret.log_level.0).init();

	ret
}