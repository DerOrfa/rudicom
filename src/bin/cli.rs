use anyhow::{Context, Result};
use std::path::PathBuf;

use clap::Parser;
use rudicom::{db,config,file::import_glob};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // file or globbing to open
    filename: PathBuf,
    // config file
    config: Option<PathBuf>,
    // url of the database to connect to
    #[arg(default_value_t = String::from("ws://localhost:8000"))]
    database: String,
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    config::init(args.config)?;
    db::init(args.database.as_str()).await.context(format!("Failed connecting to {}",args.database))?;

    let pattern = args.filename.to_str().expect("Invalid string");
    import_glob(pattern).await;
    Ok(())
}
