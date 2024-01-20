use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::tools;
use surrealdb::sql;
use crate::db::Entry;
use crate::db::DBErr;

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
    pub(crate) fn into_path(self) -> PathBuf
    {
        if self.owned { tools::complete_filepath(&self.path.as_str()) }
        else { PathBuf::from(self.path.to_raw()) }
    }
}

impl TryFrom<sql::Value> for File
{
    type Error = DBErr;

    fn try_from(value: sql::Value) -> Result<Self, Self::Error> {
        let context = format!("parsing database object {value} as File object");
        let json = value.into_json();
        serde_json::from_value(json)
            .map_err(|e|DBErr::context_from(e,context))
    }
}

impl TryFrom<File> for sql::Object
{
    type Error = DBErr;

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
    type Error = DBErr;

    fn try_from(entry: Entry) -> Result<Self, Self::Error> {
        let context= format!("trying to extract a File object from {}",entry.id());
        let result = if let Entry::Instance((id,mut inst)) = entry
        {
            inst.remove("file")
                .ok_or(DBErr::ElementMissing{element:"file".into(), parent:id.to_raw()})?
                .try_into()
        } else {Err(DBErr::UnexpectedEntry {expected:"instance".into(),id:entry.id().clone()})};
        result.map_err(|e|e.context(context))
    }
}
