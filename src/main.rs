use std::path::Path;
use dicom::object::open_file;
use anyhow::Result;
use dicom::dictionary_std::tags;

#[derive(Debug)]
struct InstanceEntry {
    instance_uid : String,
    series_uid : String
}

fn read_dicom<P>(filename:P) -> Result<InstanceEntry> where P:AsRef<Path>
{
    let obj = open_file(filename)?;
    Ok(InstanceEntry {
        instance_uid : obj.element(tags::SOP_INSTANCE_UID)?.to_str()?.to_string(),
        series_uid : obj.element(tags::SERIES_INSTANCE_UID)?.to_str()?.to_string()
    })
}

fn main() -> Result<()> {
    // "/Users/enrico/ownCloud.gwdg/38753.01_230525_123610/S1_localizer/MR.1.3.12.2.1107.5.2.0.79025.2023052512364992600000028.ima"
    let obj = read_dicom("/data/pt_gr_weiskopf_7t-mri-imagedata/2023/38906.09/220422_142019/S10_mfc_seste_b1map_v1a_scan/1.3.12.2.1107.5.2.0.79025.2022042214530632634114112.dcm")?;

    Ok(println!("{obj:?}"))
}
