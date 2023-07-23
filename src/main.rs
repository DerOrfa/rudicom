mod async_store;
mod db;
mod dicom_serde;

use std::collections::HashMap;
use std::path::Path;
use dicom::object::{DefaultDicomObject, open_file, StandardDataDictionary};
use dicom::core::{DataDictionary, DataElement, Tag};
use anyhow::{Context, Result};
use dicom::dictionary_std::tags;
use std::path::PathBuf;
use std::str::FromStr;
use clap::Parser;
use dicom::object::mem::InMemElement;
use glob::glob;
use serde::{Serialize, Serializer};


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

// fn read_dicom<P>(filename:P, instance_extract:HashMap<&str,Tag>, series_extract:HashMap<&str,Tag>, study_extract:HashMap<&str,Tag>)
//     -> Result<InstanceEntry> where P:AsRef<Path>
// {
//     let obj = open_file(filename)?;
//     let instance_meta = extract(&obj, instance_extract)?;
//     let series_meta = extract(&obj, series_extract)?;
//     let study_meta = extract(&obj, study_extract)?;
//
//     Ok(InstanceEntry {
//         instance : Entry{
//             uid: obj.element(tags::SOP_INSTANCE_UID)?.to_str()?.to_string(),
//             meta: instance_meta.into_iter()
//                 .map(|(k,v)|(k,v.cloned()))
//                 .collect()
//         },
//         series: Entry {
//             uid:obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?.to_string(),
//             meta: series_meta.into_iter()
//                 .map(|(k,v)|(k,v.cloned()))
//                 .collect()
//         },
//         study : Entry{
//             uid:obj.element(tags::STUDY_INSTANCE_UID)?.to_str()?.to_string(),
//             meta: study_meta.into_iter()
//                 .map(|(k,v)|(k,v.cloned()))
//                 .collect()
//         },
//     })
// }

#[tokio::main]
async fn main() -> Result<()>
{
    let args = Cli::parse();
    db::init("ws://localhost:8000").await.context(format!("Failed connecting to ws://localhost:8000"))?;

    let pattern = args.filename.to_str().expect("Invalid string");
    for entry in glob(pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                let obj = async_store::read_file(path).await?;
                let uid = &*obj.element(tags::SOP_INSTANCE_UID)?.to_str()?;
                let extracted = extract_by_name(&obj,vec!["OperatorsName"])?;
                match db::register(uid,extracted).await {
                    Ok(_) => {}
                    Err(e) => {println!("Failed to register {uid}:{e}")}
                }

            },
            Err(e) => println!("{e:?}")
        }
    }
    Ok(())
}
