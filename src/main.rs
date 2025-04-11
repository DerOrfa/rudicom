#![recursion_limit = "512"]
mod storage;
mod db;
mod dcm;
mod tools;
mod server;
mod config;
mod cli;

use futures::StreamExt;
use tools::import::import_glob_as_text;
use tokio::net::TcpListener;
use crate::cli::Commands;
use crate::db::DB;
use crate::tools::Context;
use crate::tools::import::ImportConfig;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::main]
async fn main() -> tools::Result<()>
{
	#[cfg(feature = "dhat-heap")]
	let _profiler = dhat::Profiler::new_heap();

	let args = cli::parse();
	config::init(args.config)?;
	if let Some(database) = args.endpoint.database{
		db::init_remote(database.as_str()).await
			.context(format!("Failed connecting to {}", database))?;
	} else if let Some(file) = args.endpoint.file {
		let file = file.canonicalize()
			.context(format!("Failed canonicalize database path {}", file.display()))?;
		db::init_file(file.as_path()).await
			.context(format!("Failed opening {}", file.display()))?;
	} else {
		db::init_local("memory").await
			.context("Failed opening in-memory db".to_string())?;
	}
	DB.use_ns("namespace").use_db("database").await?;

	match args.command {
		Commands::Server{address} => {
		DB.query(include_str!("db/init.surql")).await?;

		let inf= server::server_info().await;
		tracing::info!("database version is {}",inf.db_version);
		tracing::info!("storage path is {}",inf.storage_path);
			
		let mut set= tokio::task::JoinSet::new();
			for a in address{
				let bound = TcpListener::bind(a).await?;
				set.spawn(server::serve(bound));
			}
			set.join_all().await.into_iter().collect::<Result<Vec<_>,_>>()?;
		}
		Commands::Import{ echo_existing, echo_imported, mode, pattern } =>	{
			let config = ImportConfig{ echo: echo_imported, echo_existing };
			DB.query(include_str!("db/init.surql")).await?;
			for glob in pattern {
				let stream = import_glob_as_text(glob, config.clone(), mode.clone())?;
				//filter doesn't do unpin, so we have to nail it down here
				let mut stream = Box::pin(stream);
				while let Some(result) = stream.next().await {
					match result {
						Ok(result) => println!("{result}"),
						Err(e) => eprintln!("{e}")
					}
				}
			}
		}
		Commands::WriteConfig { file } => {
			config::write(file)?
		}
		Commands::Restore { file } => {
			tracing::info!("Restoring database from {}", file.display());
			DB.import(file).await?
		}
	}
	Ok(())
}
