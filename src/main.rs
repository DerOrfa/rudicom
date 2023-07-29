mod async_store;
mod db;

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::io::Write;
use dicom::object::{DefaultDicomObject, StandardDataDictionary};
use dicom::core::{DataDictionary, Tag};
use anyhow::{Context, Result};
use dicom::dictionary_std::tags;
use std::path::PathBuf;
use std::str::FromStr;
use clap::Parser;
use dicom::object::mem::InMemElement;
use glob::glob;
use surrealdb::sql;
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

fn prepare_meta_for_db(obj:&DefaultDicomObject, names: Vec<&str>, table:&str, id_tag:Tag) -> Result<BTreeMap<String,sql::Value>>{
    let id = obj.element(id_tag)?.to_str()?;
    let meta:BTreeMap<_,_> =
        extract_by_name(obj,names)?.into_iter()
            .map(|(k,v)|{(k,v.cloned().into_db_value())})
            .chain([
                (String::from("id"),sql::Value::Thing(sql::Thing::from((table,id.as_ref()))))
            ])
            .collect();
    Ok(meta)
}

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    let mut tasks=tokio::task::JoinSet::new();
    db::init("ws://localhost:8000").await.context(format!("Failed connecting to ws://localhost:8000"))?;

    let pattern = args.filename.to_str().expect("Invalid string");
    for entry in glob(pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                let file = async_store::read_file(path.clone()).await?;

                let instance_meta = prepare_meta_for_db(&file,vec![],"instances",tags::SOP_INSTANCE_UID)?;
                let series_meta = prepare_meta_for_db(&file,vec![],"series",tags::SERIES_INSTANCE_UID)?;
                let study_meta = prepare_meta_for_db(&file,vec!["OperatorsName"],"studies",tags::STUDY_INSTANCE_UID)?;

                tasks.spawn(db::register(instance_meta, series_meta, study_meta));
            },
            Err(e) => println!("{e:?}")
        }
    }

    while let Some(res) = tasks.join_next().await {
        if res??.is_created() {
            print!("#");
        } else {
            print!(".");
        }
        io::stdout().flush().unwrap();
    }
    println!("");
    Ok(())
}
