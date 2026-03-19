mod cli;

use futures::StreamExt;
use rudicom::tools::import::import_glob_as_text;
use tokio::net::TcpListener;
use tracing::info;
use crate::cli::Commands;
use rudicom::db::DB;
use rudicom::db;
use rudicom::tools::import::ImportConfig;
use rudicom::config;
use rudicom::server;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::main]
async fn main() -> Result<(),String>
{
	#[cfg(feature = "dhat-heap")]
	let _profiler = dhat::Profiler::new_heap();

	let args = cli::parse();
	if let	Commands::WriteConfig{ file } = args.command {
		config::write(&file)
			.map_err(|e|format!("Failed writing config file {}:{e}", file.display()))?;
		info!("Config file written to {}", file.display());
		return Ok(());
	}
	config::init(args.config).map_err(|e|e.to_string())?;

	let storage_path = &config::get().paths.storage_path;
	if !storage_path.is_absolute(){
		Err(format!("{} (the storage path) must be an absolute path",storage_path.display()))?;
	} else if !storage_path.exists(){
		Err(format!("{} (the storage path) must exist",storage_path.display()))?
	}

	if let Some(database) = args.endpoint.database{
		db::init_remote(database.as_str()).await
			.map_err(|e|format!("Failed connecting to {}: {e}", database))?;
	} else if let Some(file) = args.endpoint.file {
		let file = file.canonicalize()
			.map_err(|e|format!("Failed canonicalize database path {}: {e}", file.display()))?;
		db::init_file(file.as_path()).await
			.map_err(|e|format!("Failed opening {}: {e}", file.display()))?;
	} else {
		db::init_local("memory").await
			.map_err(|e|format!("Failed opening in-memory db {e}"))?;
	}
	DB.use_ns("namespace").use_db("database").await
		.map_err(|e|format!("Selecting database and namespace failed: {e}"))?;

	match args.command {
		Commands::Server{address} => {
			DB.query(include_str!("db/init.surql")).await
				.map_err(|e|format!("database initialisation failed: {e}"))?;

			let inf= server::server_info().await;
			info!("database version is {}",inf.db_version);
			info!("storage path is {}",inf.storage_path);

			let mut set= tokio::task::JoinSet::new();
			for a in address{
				let bound = TcpListener::bind(&a).await
					.map_err(|e|format!("Binding to {a} failed: {e}"))?;
				set.spawn(async {server::serve(bound).await.map(|_|a)});
			}
			for result in set.join_all().await{
				match result {
					Ok(a) => info!("{a} closed"),
					Err(e) => Err(format!("Server failed: {e}"))?
				}
			}
		}
		Commands::Import{ echo_existing, echo_imported, mode, pattern } =>	{
			let config = ImportConfig{ echo: echo_imported, echo_existing };
			DB.query(include_str!("db/init.surql")).await
				.map_err(|e|format!("database initialisation failed: {e}"))?;
			for glob in pattern {
				let stream = import_glob_as_text(&glob, config.clone(), mode.clone())
					.map_err(|e|format!("Importing {glob} failed:{e}"))?;
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
		Commands::Restore { file } => {
			info!("Restoring database from {}", file.display());
			DB.import(&file).await
				.map_err(|e|format!("Importing {} failed {e}",file.display()))?
		}
		Commands::WriteConfig { .. } => {unreachable!()}
	}
	Ok(())
}
