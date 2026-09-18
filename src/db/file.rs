use std::io::{Read, Write};
use std::path::PathBuf;
use crate::db::Pickable;
use crate::storage::async_store::compute_md5;
use crate::tools::{complete_filepath, Context, Error, Result};
use dicom::object::DefaultDicomObject;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use surrealdb::types as db_types;
use tokio::task::spawn_blocking;
use tracing::log::warn;
use crate::storage::checked_load;

pub(crate) struct Md5Proxy<'a,R> where R: Sized
{
	pub(crate) context:&'a mut md5::Context,
	pub(crate) inner:R,
}

impl<'a,T> Read for Md5Proxy<'a, T> where T: Read + Sized
{
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		let s = self.inner.read(buf)?;
		self.context.consume(&buf[..s]);
		if s == 0 {self.context.flush()?;}
		Ok(s)
	}
}
impl<'a,T> Write for Md5Proxy<'a, T> where T: Write + Sized
{
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		let s = self.inner.write(buf)?;
		self.context.consume(&buf[..s]);
		Ok(s)
	}

	fn flush(&mut self) -> std::io::Result<()> {
		self.inner.flush()
			.and_then(|_| self.context.flush())
	}
}

#[derive(Clone,Deserialize)]
pub struct FileInfo
{
	path:PathBuf,
	pub owned:bool,
	md5:String,
	pub size:u64
}

impl FileInfo {
	pub fn new<T>(path:T, md5:md5::Digest, owned:bool, size:u64) -> FileInfo
	where PathBuf:From<T>
	{
		let path = PathBuf::from(path);
		FileInfo {path,size, owned, md5:format!("{:x}", md5)}
	}

	/// get the complete path of the file
	/// - attaches "storage_path" from the config if the file is owned and the path is relative
	/// - as non-owned files are guaranteed to be absolute already and "storage_path" is guaranteed to be absolute, the result is always guaranteed to be absolute
	pub fn get_path(&self) -> PathBuf
	{
		if self.owned { complete_filepath(&self.path) }
		else { self.path.to_path_buf() }
	}
	pub fn get_md5(&self) -> &str { self.md5.as_str() }

	/// read the file stored at path, check its checksum and return it as dicom object
	pub async fn read(&self) -> Result<DefaultDicomObject>
	{
		let (obj, md5) = checked_load(self.get_path()).await?;
		if format!("{:x}", md5) != self.md5
		{
			let file = self.get_path().to_string_lossy().to_string();
			return Err(Error::ChecksumErr {checksum:self.md5.clone(),file});
		}
		Ok(obj)
	}

	pub async fn verify(&self) -> Result<()>
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

	pub async fn remove(self) -> Result<()>{
		if self.owned {
			let mut path = self.get_path();
			if path.exists() {
				std::fs::remove_file(&path).context(format!("deleting {}", path.display()))?;
				if path.pop(){// if there is a parent path, try to delete it as far as possible
					let ctx = format!("deleting {}",path.display());
					spawn_blocking(||crate::tools::remove::remove_path(path, &crate::config::get().paths.storage_path))
						.await.unwrap().context(ctx)?;
				}
			} else {
				warn!("trying to delete file {} but it does not exist",path.to_string_lossy())
			}
		}
		Ok(())
	} 
}

impl TryFrom<db_types::Value> for FileInfo
{
	type Error = Error;

	fn try_from(obj: db_types::Value) -> std::result::Result<Self, Self::Error> {
		let context=format!("parsing database object {obj:?} as File object");
		let kind = obj.kind().to_string();
		match obj {
			db_types::Value::Object(obj) => obj.try_into(),
			_ => Err(Error::UnexpectedResult {expected:"object".into(),found:kind})
		}.context(context)
	}
}

impl TryFrom<FileInfo> for db_types::Value
{
	type Error = Error;

	fn try_from(file: FileInfo) -> std::result::Result<Self, Self::Error> {
		let mut ret=db_types::Object::default();
		let file_path = file.path.to_str().ok_or(Error::InvalidFilename {name:file.path.clone()})?;
		ret.insert("path",file_path.to_string());
		ret.insert("owned",file.owned);
		ret.insert("md5",file.md5);
		ret.insert("size",file.size);
		Ok(ret.into())
	}
}

impl Serialize for FileInfo
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

impl TryFrom<db_types::Object> for FileInfo
{
	type Error = Error;

	fn try_from(mut obj: db_types::Object) -> std::result::Result<Self, Self::Error> {
		let path = obj.pick_remove("path")?.into_string()?;
		let owned = obj.pick_remove("owned")?.is_true();
		let md5 = obj.pick_remove("md5")?.into_string()?;
		let size = obj.pick_remove("size")
			.map(|v|if let db_types::Value::Number(num) = v { num.to_int().unwrap_or_default()} else {0})?;
		Ok(FileInfo {path:path.into(),owned,md5,size:size as u64})
	}
}
