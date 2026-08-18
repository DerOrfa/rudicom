use crate::dcm::gen_filepath;
use crate::storage::file::{Committable, CompatibleFile};
use crate::tools;
use crate::tools::Error::DicomError;
use crate::tools::{Context, complete_filepath};
use dicom::object::{DefaultDicomObject, from_reader};
use md5::Digest;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::spawn_blocking;
use tracing::warn;
use crate::db::FileInfo;

enum Image<C> where C: Committable,
{
	/// An object ready to be created as a file
	Create{
		obj:DefaultDicomObject
	},
	/// An already existing file, its object and parameters
	Existing{
		path:PathBuf,
		owned:bool,
		size:u64,
		checksum:String,
		obj:DefaultDicomObject
	},
	/// A file has been created, not yet commited
	/// dropping this will cause a rollback (aka file will be removed)
	Created{
		committable:C,
		size:u64,
		checksum:String,
	},
}

impl<C> Image<C> where C:Committable + 'static {
	/// writes a new file taking an object and returning that object plus a file info
	pub async fn new_from_obj(obj:Arc<DefaultDicomObject>) -> tools::Result<Self> {
		let path=PathBuf::from(gen_filepath(&obj)?);
		let path = complete_filepath(&path);
		let p=path.parent().unwrap();
		tokio::fs::create_dir_all(p).await
			.context(format!("Failed creating storage path {}",p.display()))?;

		let path_clone=path.clone();
		let (committable,checksum) = spawn_blocking(move || {
			let inner = C::create(path_clone)?;
			let mut checksum = md5::Context::new();
			let mut writer = crate::db::file::Md5Proxy {context:&mut checksum,inner};
			obj.write_all(&mut writer).map_err(|e|DicomError(e.into()))?;
			Ok::<_, tools::Error>((writer.inner,checksum))
		}).await??;
		let size = std::fs::metadata(&path)?.len();
		Ok(Self::Created{
			committable,
			checksum:format!("{:x}", checksum.finalize()),
			size
		})
	}
	/// creates fileinfo struct and reads dicom object directly from path
	pub async fn new_from_existing<P:AsRef<Path>>(path:P, owned:bool) -> tools::Result<Self>
	{
		let path = path.as_ref();
		let size = tokio::fs::metadata(path).await.context(format!("getting metadata for {}",path.display()))?.len();
		let reader_ctx = format!("reading {}", path.display());
		let reader = std::fs::File::open(path).context(format!("opening {}",path.display()))?;

		let obj_task= spawn_blocking(move||{
			let mut md5_context = md5::Context::new();
			let reader = crate::db::file::Md5Proxy {context:&mut md5_context,inner:reader};
			(from_reader(reader), md5_context)
		});

		let (obj,md5_context) = obj_task.await?;
		Ok(Image::Existing {
			path:path.to_path_buf(),
			owned, size,
			checksum: format!("{:x}", md5_context.finalize()),
			obj: obj.map_err(|e|DicomError(e.into())).context(reader_ctx)?,
		})
	}
	async fn load(info:&FileInfo) -> tools::Result<Self>
	{
		let image = Self::new_from_existing(info.get_path(), info.owned).await?;
		if let Image::Existing { path, owned, size, checksum, obj } = &image
		{
			if *size != info.size {
				warn!("Image size mismatch: filesize: {} != image size: {}", info.size, size);
			}
			if checksum != info.get_md5(){
				return Err(tools::Error::ChecksumErr { checksum:checksum.clone(), file: path.to_string_lossy().to_string() })
			}
		} else { unreachable!(); }
		Ok(image)
	}
	fn owned(&self) -> bool {
		match self {
			Image::Existing { owned , .. } => *owned,
			Image::Create { .. } | Image::Created { .. } => true,
		}
	}
}

impl From<DefaultDicomObject> for Image<CompatibleFile<std::fs::File>> {
	fn from(obj:DefaultDicomObject) -> Self {Self::Create {obj}}
}

impl<C> AsRef<DefaultDicomObject> for Image<C> where C:Committable {
	fn as_ref(&self) -> &DefaultDicomObject {
		match self {
			Image::Create {obj, .. }
			| Image::Existing {obj, ..} => obj,
			Image::Created {..} => panic!("Invalid object reference on created image file")
		}
	}
}