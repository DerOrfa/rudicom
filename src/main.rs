use std::collections::HashMap;
use std::path::Path;
use dicom::object::{DefaultDicomObject, open_file, StandardDataDictionary};
use dicom::core::Tag;
use anyhow::Result;
use dicom::dictionary_std::tags;
use std::path::PathBuf;
use clap::Parser;
use dicom::object::mem::InMemElement;


#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // file to open
    filename: PathBuf,
}

#[derive(Debug)]
struct Entry{
    uid: String,
    meta: HashMap<String,InMemElement<StandardDataDictionary>>
}

#[derive(Debug)]
struct InstanceEntry{
    instance : Entry,
    series : Entry,
    study : Entry
}

fn extract_dicom(obj:&DefaultDicomObject, requested:HashMap<String,Tag>) -> Result<HashMap<String,&InMemElement<StandardDataDictionary>>>
{
    requested.into_iter()
        .map(|(k,v)|
            match obj.element(v) {
                Ok(el) => Ok((k,el)),
                Err(e) => Err(anyhow::Error::from(e))
            }
        )
        .collect()
}

fn read_dicom<P>(filename:P, instance_extract:HashMap<String,Tag>, series_extract:HashMap<String,Tag>, study_extract:HashMap<String,Tag>)
    -> Result<InstanceEntry> where P:AsRef<Path>
{
    let obj = open_file(filename)?;

    Ok(InstanceEntry {
        instance : Entry{
            uid: obj.element(tags::SOP_INSTANCE_UID)?.to_str()?.to_string(),
            meta: extract_dicom(&obj,instance_extract)?.into_iter().map(|(k,v)|(k,v.clone())).collect()
        },
        series: Entry {
            uid:obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?.to_string(),
            meta: extract_dicom(&obj,series_extract)?.into_iter().map(|(k,v)|(k,v.clone())).collect()
        },
        study : Entry{
            uid:obj.element(tags::STUDY_INSTANCE_UID)?.to_str()?.to_string(),
            meta: extract_dicom(&obj,study_extract)?.into_iter().map(|(k,v)|(k,v.clone())).collect()
        },
    })
}

fn main() -> Result<()>
{
    let args = Cli::parse();
    // "/data/pt_gr_weiskopf_7t-mri-imagedata/2023/38906.09/220422_142019/S10_mfc_seste_b1map_v1a_scan/1.3.12.2.1107.5.2.0.79025.2022042214530632634114112.dcm"
    let obj = read_dicom(args.filename, [].into(), [].into(), [].into() )?;

    Ok(println!("{obj:#?}"))
}
