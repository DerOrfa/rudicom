#![recursion_limit = "512"]
mod storage;
mod db;
mod dcm;
mod tools;
mod server;
mod config;

use std::path::PathBuf;
use futures::StreamExt;
use clap::ValueHint::Hostname;

use clap::{Args, Parser, Subcommand};
use tools::import::import_glob_as_text;
use tokio::net::TcpListener;
use crate::tools::Context;

#[cfg(feature = "embedded")]
use clap::ValueHint::DirPath;

#[derive(Args,Debug)]
#[group(required = true, multiple = false)]
struct Endpoint{
    /// hostname of the database
    #[arg(long, value_hint = Hostname)]
    database: Option<String>,
    /// filename for the local database
    #[cfg(feature = "embedded")]
    #[arg(long, value_hint = DirPath)]
    file:Option<PathBuf>
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// config file
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(flatten)]
    endpoint: Endpoint
}

#[derive(Subcommand)]
enum Commands {
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

#[tokio::main]
async fn main() -> crate::tools::Result<()>
{
    let args = Cli::parse();
    config::init(args.config)?;
    if let Some(database) = args.endpoint.database{
        db::init_remote(database.as_str()).await
            .context(format!("Failed connecting to {}", database))?;
    } else {
        #[cfg(feature = "embedded")]
        if let Some(file) = args.endpoint.file {
            db::init_local(file.as_path()).await
                .context(format!("Failed opening {}", file.to_string_lossy()))?;
        } else {
            println!("No data backend, go away..");
            return Ok(());
        }
        #[cfg(not(feature = "embedded"))]
        {println!("No data backend, go away..");return Ok(());}
    }

    match args.command {
        Commands::Server{address} => {
            let bound = TcpListener::bind(address).await?;
            server::serve(bound).await?;
        }
        Commands::Import{ existing, imported, pattern } => {
            let stream=import_glob_as_text(pattern,imported,existing)?;
            //filter doesn't do unpin, so we have to nail it down here
            let mut stream=Box::pin(stream);
            while let Some(result)=stream.next().await {
                match result {
                    Ok(result) => println!("{result}"),
                    Err(e) => eprintln!("{e}")
                }
            }
        }
        // Commands::Remove {id} => {
        //     let id=thing(id.as_str()).context(format!("Failed to parse database id {id}"))?;
        //     tools::remove::remove(id).await?;
        // }
        Commands::WriteConfig { file } => {
            config::write(file)?
        }
    }
    Ok(())
}
