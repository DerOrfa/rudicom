use std::io;
use std::io::Cursor;
use std::path::PathBuf;

use dicom::object::{DefaultDicomObject, from_reader};
use serde::{Deserialize, Serialize, Serializer};
use serde::ser::SerializeStruct;
use surrealdb::sql;
use tokio::io::AsyncReadExt;

use crate::db::{Entry, get_from_object};
use crate::storage::async_store::compute_md5;
use crate::tools::{complete_filepath, Context, Error, Result};

#[derive(Deserialize)]
pub struct File
{
    path:PathBuf,
    pub owned:bool,
    md5:String,
    pub size:u64
}

impl File {
    /// create file info for an owned file
    /// - the file does not need to exist
    pub(crate) fn from_owned<T>(path:T, md5:md5::Digest, size:u64) -> File where PathBuf:From<T>
    {
        let path = PathBuf::from(path);
        File{path,size:size, owned:true, md5:format!("{:x}", md5)}
    }
    /// create file info for a not owned file
    /// - the file must exist
    /// - the path will be canonicalized
    pub(crate) fn from_unowned<T>(path:T, md5:md5::Digest) -> io::Result<File> where PathBuf:From<T>
    {
        let path = PathBuf::from(path).canonicalize()?;
        let size = path.as_path().metadata()?.len().into();
        Ok(File{path,size,owned:false,md5:format!("{:x}", md5)})
    }

    /// get the complete path of the file
    /// - attaches "storage_path" from the config if the file is owned and the path is relative
    /// - as non-owned files are guaranteed to be absolute already and "storage_path" is guaranteed to be absolute, the result is always guaranteed to be absolute
    pub(crate) fn get_path(&self) -> PathBuf
    {
        if self.owned { complete_filepath(&self.path) }
        else { self.path.to_path_buf() }
    }
    pub(crate) fn get_md5(&self) -> &str { self.md5.as_str() }

    /// read the file stored at path, check its checksum and return it as dicom object
    pub(crate) async fn read(&self) -> Result<DefaultDicomObject>
    {
        let mut buffer = Vec::<u8>::new();
        tokio::fs::File::open(self.get_path().as_path()).await?.read_to_end(&mut buffer).await?;

        let checksum = md5::compute(buffer.as_slice());
        if format!("{:x}",checksum) != self.md5
        {
            let file = self.get_path().to_string_lossy().to_string();
            return Err(Error::ChecksumErr {checksum:self.md5.clone(),file});
        }
        from_reader(Cursor::new(buffer)).map_err(|e|Error::DicomError(e.into()))
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
    type Error = Error;

    fn try_from(obj: sql::Value) -> std::result::Result<Self, Self::Error> {
        let context=format!("parsing database object {obj} as File object");
        match obj {
            sql::Value::Object(obj) => obj.try_into(),
            _ => Err(Error::UnexpectedResult {expected:"object".into(),found:obj})
        }.context(context)
    }
}

impl TryFrom<File> for sql::Object
{
    type Error = Error;

    fn try_from(file: File) -> std::result::Result<Self, Self::Error> {
        let mut ret=sql::Object::default();
        let file_path = file.path.to_str().ok_or(Error::InvalidFilename {name:file.path.clone()})?;
        ret.insert("path".into(),file_path.into());
        ret.insert("owned".into(),file.owned.into());
        ret.insert("md5".into(),file.md5.into());
        ret.insert("size".into(),file.size.into());
        Ok(ret)
    }
}

impl Serialize for File
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> where S: Serializer {
        let mut ser = serializer.serialize_struct("file",3)?;
        let file_path = self.path.to_str()
            .ok_or(Error::InvalidFilename {name:self.path.clone()})
            .map_err(serde::ser::Error::custom)?;
        ser.serialize_field("path",file_path)?;
        ser.serialize_field("owned",&self.owned)?;
        ser.serialize_field("md5",self.md5.as_str())?;
        ser.serialize_field("size",&self.size)?;
        ser.end()
    }
}

impl TryFrom<sql::Object> for File
{
    type Error = Error;

    fn try_from(obj: sql::Object) -> std::result::Result<Self, Self::Error> {
        let path = get_from_object(&obj,"path").map(|v| v.clone().as_raw_string())?;
        let owned = get_from_object(&obj,"owned").map(|v|v.is_true())?;
        let md5 = get_from_object(&obj,"md5").map(|v|v.clone().as_raw_string())?;
        let size = get_from_object(&obj,"size")
            .map(|v|if let sql::Value::Number(num) = v { num.to_int()} else {0})?;
        Ok(File{path:path.into(),owned,md5,size:size as u64})
    }
}

impl TryFrom<Entry> for File
{
    type Error = Error;

    fn try_from(entry: Entry) -> std::result::Result<Self, Self::Error> {
        let context= format!("trying to extract a File object from {}",entry.id());
        let result = if let Entry::Instance((id,mut inst)) = entry
        {
            inst.remove("file")
                .ok_or(Error::ElementMissing{element:"file".into(), parent:id.to_raw()})?
                .try_into()
        } else {Err(Error::UnexpectedEntry {expected:"instance".into(),id:entry.id().clone()})};
        result.context(context)
    }
}
