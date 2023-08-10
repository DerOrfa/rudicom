use anyhow::{Context, Result};
use std::path::PathBuf;

use clap::{Parser,Subcommand};
use rudicom::{db, config, file::import_glob,tools};
use surrealdb::sql::thing;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    // config file
    #[arg(long)]
    config: Option<PathBuf>,
    // url of the database to connect to
    #[arg(long,default_value_t = String::from("ws://localhost:8000"))]
    database: String,
}

#[derive(Subcommand)]
enum Commands {
    Import {
        // file or globbing to open
        pattern: PathBuf,
    },
    Remove{
        // database id of the object to delete
        id:String
    }
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    config::init(args.config)?;
    db::init(args.database.as_str()).await.context(format!("Failed connecting to {}",args.database))?;

    match &args.command {
        Commands::Import{pattern} => {
            let pattern = pattern.to_str().expect("Invalid string");
            import_glob(pattern).await;
        }
        Commands::Remove {id} => {
            let id=thing(id).context(format!("Failed to parse database id {id}"))?;
            tools::remove::remove(id).await?;
        }
    }
    Ok(())
}
