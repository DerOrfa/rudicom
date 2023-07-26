mod async_store;
mod db;

use std::collections::{BTreeMap, HashMap};
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use dicom::core::{DataDictionary, Tag};
use anyhow::{Context, Result};
use dicom::dictionary_std::tags;
use std::path::PathBuf;
use std::str::FromStr;
use clap::Parser;
use dicom::object::mem::InMemElement;
use glob::glob;
use crate::db::IntoDbValue;


#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // file or globbing to open
    filename: PathBuf,
}

fn extract_by_name<'a>(obj:&'a DefaultDicomObject, names: Vec<&str>) -> Result<HashMap<String,Option<&'a InMemElement>>>
{
    let mut request = HashMap::new();
    for name in names{
        let tag = StandardDataDictionary::default()
            .by_name(name)
            .map(|t|t.tag.inner())
            .or_else(||Tag::from_str(name).ok())
            .ok_or(anyhow::Error::msg(format!("Tag {name} not found")))?;

        request.insert(name, tag);
    }
    extract(obj,request)
}

fn extract<'a>(obj:&'a DefaultDicomObject, requested:HashMap<&str,Tag>) -> Result<HashMap<String,Option<&'a InMemElement>>>
{
    let mut ret = HashMap::new();
    for (key,tag) in requested{
        let found = obj.element_opt(tag)?;
        ret.insert(key.into(),found);
    }

    Ok(ret)
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    db::init("ws://localhost:8000").await.context(format!("Failed connecting to ws://localhost:8000"))?;

    let pattern = args.filename.to_str().expect("Invalid string");
    for entry in glob(pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                let file = async_store::read_file(path.clone()).await?;
                let extracted:BTreeMap<_,_> =
                    extract_by_name(&file,vec!["OperatorsName"])?.into_iter()
                        .map(|(k,v)|{(k,v.cloned().into_db_value())})
                        .collect();
                let uid = file.element(tags::SOP_INSTANCE_UID)?.to_str()?;
                db::register_instance(uid.as_ref(),extracted).await?;
            },
            Err(e) => println!("{e:?}")
        }
    }
    Ok(())
}
