use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::db::Pickable;
use crate::storage::async_store::compute_md5;
use crate::tools::{complete_filepath, Context, Error, Result};
use dicom::object::{from_reader, DefaultDicomObject};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use surrealdb::sql;
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
pub struct File
{
	path:PathBuf,
	pub owned:bool,
	md5:String,
	pub size:u64
}

impl File {
	pub(crate) fn new<T>(path:T, md5:md5::Digest, owned:bool, size:u64) -> File where PathBuf:From<T>
	{
		let path = PathBuf::from(path);
		File{path,size, owned, md5:format!("{:x}", md5)}
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

	/// creates fileinfo struct and reads dicom object directly from path
	pub(crate) async fn new_from_existing<P:AsRef<Path>>(path:P, owned:bool) -> Result<(File,DefaultDicomObject)>
	{
		let mut buffer = Vec::<u8>::new();
		let size= tokio::fs::File::open(path.as_ref()).await?.read_to_end(&mut buffer).await?;

		let checksum = md5::compute(buffer.as_slice());

		let obj= from_reader(Cursor::new(buffer)).map_err(|e|Error::DicomError(e.into()))?;
		Ok((Self::new(path.as_ref(),checksum,owned,size as u64), obj))
	}

	/// read the file stored at path, check its checksum and return it as dicom object
	pub(crate) async fn read(&self) -> Result<DefaultDicomObject>
	{
		let (red_info,obj) = Self::new_from_existing(self.get_path(),self.owned).await?;
		if red_info.md5 != self.md5
		{
			let file = self.get_path().to_string_lossy().to_string();
			return Err(Error::ChecksumErr {checksum:self.md5.clone(),file});
		}
		Ok(obj)
	}

	pub(crate) async fn verify(&self) -> Result<()>
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

impl TryFrom<surrealdb::Value> for File
{
	type Error = Error;

	fn try_from(obj: surrealdb::Value) -> std::result::Result<Self, Self::Error> {
		let context=format!("parsing database object {obj} as File object");
		let obj = obj.into_inner();
		let kind = obj.kindof();
		match obj {
			sql::Value::Object(obj) => surrealdb::Object::from_inner(obj).try_into(),
			_ => Err(Error::UnexpectedResult {expected:"object".into(),found:kind})
		}.context(context)
	}
}

impl TryFrom<File> for surrealdb::Value
{
	type Error = Error;

	fn try_from(file: File) -> std::result::Result<Self, Self::Error> {
		let mut ret=sql::Object::default();
		let file_path = file.path.to_str().ok_or(Error::InvalidFilename {name:file.path.clone()})?;
		ret.insert("path".into(),file_path.into());
		ret.insert("owned".into(),file.owned.into());
		ret.insert("md5".into(),file.md5.into());
		ret.insert("size".into(),file.size.into());
		Ok(surrealdb::Value::from_inner(ret.into()))
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

impl TryFrom<surrealdb::Object> for File
{
	type Error = Error;

	fn try_from(mut obj: surrealdb::Object) -> std::result::Result<Self, Self::Error> {
		let path = obj.pick_remove("path").map(|v| v.into_inner().as_raw_string())?;
		let owned = obj.pick_remove("owned").map(|v|v.into_inner().is_true())?;
		let md5 = obj.pick_remove("md5").map(|v|v.into_inner().as_raw_string())?;
		let size = obj.pick_remove("size")
			.map(|v|if let sql::Value::Number(num) = v.into_inner() { num.to_int()} else {0})?;
		Ok(File{path:path.into(),owned,md5,size:size as u64})
	}
}
