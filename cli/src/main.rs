#![recursion_limit = "512"]
extern crate core;

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
    } else {
        #[cfg(feature = "embedded")]
        if let Some(file) = args.endpoint.file {
            let file = file.canonicalize()
                .context(format!("Failed canonicalize database path {}", file.to_string_lossy()))?;
            db::init_local(file.as_path()).await
                .context(format!("Failed opening {}", file.to_string_lossy()))?;
        } else {
            println!("No database backend, go away..");
            panic!("No database backend");
        }
        #[cfg(not(feature = "embedded"))]
        {
            eprintln!("No database backend, go away..");
            panic!("No database backend");
        }
    }

    match args.command {
        cli::Commands::Server{address} => {
            let bound = TcpListener::bind(address).await?;
            server::serve(bound).await?;
        }
        cli::Commands::Import{ echo_existing, echo_imported, store, pattern } => 
        {
            let config = ImportConfig{ echo: echo_imported, echo_existing, store };
            for glob in pattern {
                let stream = import_glob_as_text(glob, config.clone())?;
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
        cli::Commands::WriteConfig { file } => {
            config::write(file)?
        }
    }
    Ok(())
}
