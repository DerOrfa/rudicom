use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::db::Pickable;
use crate::storage::async_store::compute_md5;
use crate::tools::{complete_filepath, Context, Error, Result};
use dicom::object::{from_reader, DefaultDicomObject};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use surrealdb::types as db_types;
use tokio::task::spawn_blocking;
use crate::dcm::gen_filepath;
use crate::tools::Error::DicomError;

struct Md5Proxy<'a,R> where R: Sized
{
	context:&'a mut md5::Context,
	inner:R,
}

impl<'a,T> Read for Md5Proxy<'a, T> where T: Read + Sized
{
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		let r = self.inner.read(buf)?;
		self.context.consume(buf);
		Ok(r)
	}
}
impl<'a,T> Write for Md5Proxy<'a, T> where T: Write + Sized
{
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.context.consume(buf);
		self.inner.write(buf)
	}

	fn flush(&mut self) -> std::io::Result<()> {
		self.inner.flush()
			.and_then(|_| self.context.flush())
	}
}

#[derive(Deserialize)]
pub struct File
{
	path:PathBuf,
	pub owned:bool,
	md5:String,
	pub size:u64
}

impl File {
	pub fn new<T>(path:T, md5:md5::Digest, owned:bool, size:u64) -> File where PathBuf:From<T>
	{
		let path = PathBuf::from(path);
		File{path,size, owned, md5:format!("{:x}", md5)}
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

	pub async fn new_from_obj(obj:DefaultDicomObject) -> Result<(DefaultDicomObject,File)>{
		let path=PathBuf::from(gen_filepath(&obj)?);
		let path = complete_filepath(&path);
		let p=path.parent().unwrap();
		tokio::fs::create_dir_all(p).await
			.context(format!("Failed creating storage path {}",p.display()))?;

		let path_clone=path.clone();
		let (obj, checksum) = spawn_blocking(move || {
			let inner=std::fs::File::create_new(&path_clone)
				.context(format!("creating file {}",path_clone.display()))?;
			let mut checksum = md5::Context::new();
			let writer = Md5Proxy{context:&mut checksum,inner};
			obj.write_all(writer).map_err(|e|DicomError(e.into())).map(|_|(obj,checksum))
		}).await?
			.context(format!("Writing object data to {}",path.display()))?;
		let size = std::fs::metadata(&path)?.len();
		Ok((obj,Self::new(path, checksum.finalize(), true,size)))
	}
	/// creates fileinfo struct and reads dicom object directly from path
	pub async fn new_from_existing<P:AsRef<Path>>(path:P, owned:bool) -> Result<(File,DefaultDicomObject)>
	{
		let path = path.as_ref();
		let size = tokio::fs::metadata(path).await.context(format!("getting metadata for {}",path.display()))?.len();
		let reader_ctx = format!("reading {}", path.display());
		let reader = std::fs::File::open(path).context(format!("opening {}",path.display()))?;

		let obj_task= tokio::task::spawn_blocking(move||{
			let mut md5_context = md5::Context::new();
			let reader = Md5Proxy{context:&mut md5_context,inner:reader};
			(from_reader(reader), md5_context)
		});

		let (obj,md5_context) = obj_task.await?;

		Ok((
			Self::new(path, md5_context.finalize(), owned, size),
			obj.map_err(|e|Error::DicomError(e.into())).context(reader_ctx)?
		))
	}

	/// read the file stored at path, check its checksum and return it as dicom object
	pub async fn read(&self) -> Result<DefaultDicomObject>
	{
		let (red_info,obj) = Self::new_from_existing(self.get_path(),self.owned).await?;
		if red_info.md5 != self.md5
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
}

impl TryFrom<db_types::Value> for File
{
	type Error = Error;

	fn try_from(obj: db_types::Value) -> std::result::Result<Self, Self::Error> {
		let context=format!("parsing database object {obj:?} as File object");
		let kind = obj.kind().to_string();
		match obj {
			db_types::Value::Object(obj) => obj.try_into(),
			_ =>
				Err(Error::UnexpectedResult {expected:"object".into(),found:kind})
		}.context(context)
	}
}

impl TryFrom<File> for db_types::Value
{
	type Error = Error;

	fn try_from(file: File) -> std::result::Result<Self, Self::Error> {
		let mut ret=db_types::Object::default();
		let file_path = file.path.to_str().ok_or(Error::InvalidFilename {name:file.path.clone()})?;
		ret.insert("path",file_path.to_string());
		ret.insert("owned",file.owned);
		ret.insert("md5",file.md5);
		ret.insert("size",file.size);
		Ok(ret.into())
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

impl TryFrom<db_types::Object> for File
{
	type Error = Error;

	fn try_from(mut obj: db_types::Object) -> std::result::Result<Self, Self::Error> {
		let path = obj.pick_remove("path")?.into_string()?;
		let owned = obj.pick_remove("owned")?.is_true();
		let md5 = obj.pick_remove("md5")?.into_string()?;
		let size = obj.pick_remove("size")
			.map(|v|if let db_types::Value::Number(num) = v { num.to_int().unwrap_or_default()} else {0})?;
		Ok(File{path:path.into(),owned,md5,size:size as u64})
	}
}
