use std::net::SocketAddr;
use anyhow::{Context, Result};
use std::path::PathBuf;

use clap::{Parser,Subcommand};
use rudicom::server;
use rudicom::{db, config};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// config file
    #[arg(long)]
    config: Option<PathBuf>,
    /// url of the database to connect to
    #[arg(long,default_value_t = String::from("ws://localhost:8000"))]
    database: String,
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
        #[arg(default_value_t = SocketAddr::from(([127, 0, 0, 1], 3000)))]
        adress: SocketAddr,
    },
    /// import (big chunks of) data from the filesystem
    Import {
        /// file or globbing to import
        pattern: PathBuf,
    },
    // Remove{
    //     // database id of the object to delete
    //     id:String
    // }
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    config::init(args.config)?;
    db::init(args.database.as_str()).await.context(format!("Failed connecting to {}",args.database))?;

    match args.command {
        Commands::Server{adress} => {
            server::serve(adress).await?;
        }
        Commands::Import{pattern} => {
            let pattern = pattern.to_str().expect("Invalid string");
            rudicom::storage::import_glob(pattern).await;
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
