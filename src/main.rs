mod db;
mod dcm;
mod file;

use anyhow::{Context, Result};
use std::path::PathBuf;

use clap::Parser;
use crate::file::import_glob;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // file or globbing to open
    filename: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    db::init("ws://localhost:8000").await.context(format!("Failed connecting to ws://localhost:8000"))?;

    let pattern = args.filename.to_str().expect("Invalid string");
    import_glob(pattern).await;
    Ok(())
}
