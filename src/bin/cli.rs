use std::net::SocketAddr;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use clap::ValueHint::{DirPath,Hostname};

use clap::{Args, Parser, Subcommand};
use rudicom::server;
use rudicom::{db, config};

#[derive(Args,Debug)]
#[group(required = true, multiple = false)]
struct Endpoint{
    /// hostname of the database
    #[arg(long, value_hint = Hostname)]
    database: Option<String>,
    /// filename for the local database
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
        #[arg(default_value_t = SocketAddr::from(([127, 0, 0, 1], 3000)))]
        address: SocketAddr,
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
    if let Some(database) = args.endpoint.database{
        db::init_remote(database.as_str()).await.context(format!("Failed connecting to {}", database))?;
    } else if let Some(file) = args.endpoint.file {
        db::init_local(file.as_path()).await.context(format!("Failed opening {}", file.to_string_lossy()))?;
    } else {
        bail!("No data backend, go away..")
    }

    match args.command {
        Commands::Server{address} => {
            server::serve(address).await?;
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
