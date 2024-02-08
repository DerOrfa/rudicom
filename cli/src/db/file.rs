use std::io;
use std::path::PathBuf;
use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;
use crate::tools;
use surrealdb::sql;
use crate::db::Entry;
use crate::storage::async_store::compute_md5;
use crate::tools::{Context, Error};

pub struct File
{
    path:PathBuf,
    pub owned:bool,
    md5:String
}

impl File {
    fn extract(obj:&mut sql::Object,key:&str) -> crate::tools::Result<sql::Value>
    {
        obj.0.remove(key)
            .ok_or(Error::ElementMissing {element:key.into(),parent:"file object".into()})
    }
    /// create file info for an owned file
    pub(crate) fn from_owned<T>(path:T, md5:md5::Digest) -> File where PathBuf:From<T>
    {
        File{path:path.into(),owned:true,md5:format!("{:x}", md5)}
    }
    /// create file info for a not owned file
    /// - the file must exist
    /// - the path will be canonicalized
    pub(crate) fn from_unowned<T>(path:T, md5:md5::Digest) -> io::Result<File> where T:Into<PathBuf>
    {
        let path=Into::into(path).canonicalize()?;
        Ok(File{path,owned:true,md5:format!("{:x}", md5)})
    }

    /// get the complete path of the file
    /// - attaches "storage_path" from the config if the file is owned and the path is relative
    /// - as non-owned files are guaranteed to be absolute already and "storage_path" is guaranteed to be absolute, the result is always guaranteed to be absolute
    pub(crate) fn get_path(&self) -> PathBuf
    {
        if self.owned { tools::complete_filepath(&self.path) }
        else { self.path.to_path_buf() }
    }
    pub(crate) fn get_md5(&self) -> &str { self.md5.as_str() }

    /// get the complete path of the file
    /// - attaches "storage_path" from the config if the file is owned and the path is relative
    /// - as non-owned files are guaranteed to be absolute already and "storage_path" is guaranteed to be absolute, the result is always guaranteed to be absolute
    pub(crate) fn into_path(self) -> PathBuf
    {
        if self.owned { tools::complete_filepath(&self.path) }
        else { self.path }
    }

    pub(crate) async fn verify(&self) -> crate::tools::Result<()>
    {
        let md5_stored = &self.md5;
        let filename = self.get_path();
        let md5_computed = format!("{:x}", compute_md5(filename.as_path()).await?);
        if &md5_computed == md5_stored {Ok(())}
        else {Err(Error::ChecksumErr{
            checksum:md5_computed,
            file:filename.to_string_lossy().into()
        })}

    }
}

impl TryFrom<sql::Value> for File
{
    type Error = crate::tools::Error;

    fn try_from(obj: sql::Value) -> Result<Self, Self::Error> {
        let context=format!("parsing database object {obj} as File object");
        match obj {
            sql::Value::Object(obj) => obj.try_into(),
            _ => Err(Error::UnexpectedResult {expected:"object".into(),found:obj})
        }.context(context)
    }
}

impl TryFrom<File> for sql::Object
{
    type Error = crate::tools::Error;

    fn try_from(file: File) -> Result<Self, Self::Error> {
        let mut ret=sql::Object::default();
        let file_path = file.path.to_str().ok_or(Error::InvalidFilename {name:file.path.clone()})?;
        ret.insert("path".into(),file_path.into());
        ret.insert("owned".into(),file.owned.into());
        ret.insert("md5".into(),file.md5.into());
        Ok(ret)
    }
}

impl Serialize for File
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut ser = serializer.serialize_struct("file",3)?;
        let file_path = self.path.to_str()
            .ok_or(Error::InvalidFilename {name:self.path.clone()})
            .map_err(serde::ser::Error::custom)?;
        ser.serialize_field("path",file_path)?;
        ser.serialize_field("owned",&self.owned)?;
        ser.serialize_field("md5",self.md5.as_str())?;
        ser.end()
    }
}

impl TryFrom<sql::Object> for File
{
    type Error = crate::tools::Error;

    fn try_from(mut obj: sql::Object) -> Result<Self, Self::Error> {
        let path = Self::extract(&mut obj,"path").map(|v| v.as_raw_string())?;
        let owned = Self::extract(&mut obj,"owned").map(|v|v.is_true())?;
        let md5 = Self::extract(&mut obj,"md5").map(|v|v.as_raw_string())?;
        Ok(File{path:path.into(),owned,md5})
    }
}

impl TryFrom<Entry> for File
{
    type Error = crate::tools::Error;

    fn try_from(entry: Entry) -> Result<Self, Self::Error> {
        let context= format!("trying to extract a File object from {}",entry.id());
        let result = if let Entry::Instance((id,mut inst)) = entry
        {
            inst.remove("file")
                .ok_or(tools::Error::ElementMissing{element:"file".into(), parent:id.to_raw()})?
                .try_into()
        } else {Err(tools::Error::UnexpectedEntry {expected:"instance".into(),id:entry.id().clone()})};
        result.context(context)
    }
}
