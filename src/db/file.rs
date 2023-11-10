use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use surrealdb::sql::Value;
use crate::tools;
use anyhow::Context;

#[derive(Serialize,Deserialize)]
pub(crate) struct File
{
    path:String,
    pub owned:bool,
    pub md5:String
}

impl File {
    pub(crate) fn path(&self) -> PathBuf
    {
        if self.owned { tools::complete_filepath(&self.path) } else { PathBuf::from(self.path.clone()) }
    }
}

impl TryFrom<Value> for File
{
    type Error = anyhow::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let json = value.into_json();
        Context::context(serde_json::from_value(json), "When reading file entry")
    }
}
