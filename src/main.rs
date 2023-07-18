use dicom::object::{Result, open_file};
use dicom::dictionary_std::tags;


fn read_dicom<P>(filename:P) -> Result<()> where P:AsRef<P>
{
    let obj = open_file(filename)?;
    let instance_uid = obj.element(tags::SOP_INSTANCE_UID)?;
    let series_uid = obj.element(tags::SERIES_INSTANCE_UID)?;

}

fn main() -> Result<()> {
    let obj = open_file("/Users/enrico/ownCloud.gwdg/38753.01_230525_123610/S1_localizer/MR.1.3.12.2.1107.5.2.0.79025.2023052512364992600000028.ima")?;
    let patient_name = obj.element(tags::PATIENT_NAME)?;
    let modality = obj.element(tags::MODALITY)?;
    let instance_uid = obj.element(tags::SOP_INSTANCE_UID)?;
    let series_uid = obj.element(tags::SERIES_INSTANCE_UID)?;

    Ok(println!("{instance_uid:?} case of patient {patient_name:?}"))
}
