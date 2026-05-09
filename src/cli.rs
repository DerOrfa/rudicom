use clap::builder::PossibleValue;
use clap::ValueHint::{FilePath, Hostname};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use tracing::Level;

use clap::ValueHint::DirPath;
#[cfg(feature = "instrumentation")]
use console_subscriber::ConsoleLayer;
use rudicom::tools::import::ImportMode;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

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
		let alias= self.0.to_string().to_lowercase();
		Some(PossibleValue::new(self.0.as_str()).alias(alias))
	}
}

#[derive(Args,Debug)]
#[group(required = false, multiple = false)]
pub(super) struct Endpoint{
	/// hostname of the database
	#[arg(long, value_hint = Hostname)]
	pub(super) database: Option<String>,
	/// filename for the local database
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
	#[arg(long, default_value = Level::WARN.as_str())]
	pub(super) log_level:LogLevel
}

#[derive(Subcommand)]
pub enum Commands {
	/// writing the default config out into the given file
	WriteConfig {file:PathBuf},
	/// run the server
	Server {
		/// ip and port to listen on
		#[arg(default_value = "127.0.0.1:3000")]
		address: Vec<String>,
	},
	/// restore database from SureQL snapshot
	Restore {file:PathBuf},
	/// import (big chunks of) data from the filesystem
	Import {
		/// report on already existing files
		#[arg(long,default_value_t=false)]
		echo_existing:bool,
		/// report on imported files
		#[arg(long="echo",default_value_t=false)]
		echo_imported:bool,
		#[arg(long, default_value_t)]
		mode:ImportMode,
		/// file or globbing to import
		pattern:Vec<String>,
	},
}


pub(super) fn parse() -> Cli
{
	let ret=Cli::parse();

	#[cfg(windows)]
	let ansi = match ansi_term::enable_ansi_support(){
		Ok(_) => true,
		Err(e) => {
			eprintln!("Failed to enable ansi color support (error code {e})");
			false
		},
	};
	#[cfg(not(windows))]
	let ansi = true;

	let tracing=tracing_subscriber::registry();
	#[cfg(feature = "instrumentation")]
	let tracing=tracing.with(ConsoleLayer::builder().with_default_env().spawn());
	tracing.with(
		tracing_subscriber::fmt::layer()
			.with_ansi(ansi)
			.with_filter(LevelFilter::from_level(ret.log_level.0))
	)
	.init();
	ret
}
