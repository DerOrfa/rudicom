use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::tools;
use anyhow::anyhow;
use surrealdb::sql;
use crate::db::Entry;

#[derive(Serialize,Deserialize)]
pub struct File
{
    pub path:sql::Strand,
    pub owned:bool,
    pub md5:sql::Strand
}

impl File {
    pub(crate) fn get_path(&self) -> PathBuf
    {
        if self.owned { tools::complete_filepath(&self.path.as_str()) }
        else { PathBuf::from(self.path.clone().to_raw()) }
    }
}

impl TryFrom<sql::Value> for File
{
    type Error = anyhow::Error;

    fn try_from(value: sql::Value) -> Result<Self, Self::Error> {
        let json = value.into_json();
        Ok(serde_json::from_value(json)?)
    }
}

impl TryFrom<File> for sql::Object
{
    type Error = anyhow::Error;

    fn try_from(file: File) -> Result<Self, Self::Error> {
        let mut ret=sql::Object::default();
        ret.insert("path".into(),file.path.into());
        ret.insert("owned".into(),file.owned.into());
        ret.insert("md5".into(),file.md5.into());
        Ok(ret)
    }
}

impl TryFrom<Entry> for File
{
    type Error = anyhow::Error;

    fn try_from(entry: Entry) -> Result<Self, Self::Error> {
        if let Entry::Instance((id,mut inst)) = entry
        {
            inst.remove("file")
                .ok_or(anyhow!(r#""file" missing in instance {id}"#))?
                .try_into()
        } else {Err(anyhow!("entry {} is not an instance",entry.id()))}
    }
}
