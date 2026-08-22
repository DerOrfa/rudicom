use crate::dcm::gen_filepath;
use crate::storage::file::{Committable, StandardFile};
use crate::tools;
use crate::tools::Error::DicomError;
use crate::tools::{Context, complete_filepath};
use dicom::object::DefaultDicomObject;
use md5::Digest;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::spawn_blocking;
use tracing::warn;
use crate::db::FileInfo;
use crate::storage::checked_load;

#[derive(Debug)]
pub enum Image<C> where C:Committable,
{
	/// An object ready to be created as a file
	Create{
		obj:DefaultDicomObject
	},
	/// An already existing file, its object and parameters
	Existing{
		path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// An already existing file, but should be moved
	Move {
		org_path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	Moved {
		org_path:Option<PathBuf>,
		committable:Result<C,PathBuf>,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// An already existing file, but should be copied
	Copy {
		org_path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	Copied {
		committable:C,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// A file has been created, not yet commited
	/// dropping this will cause a rollback (aka file will be removed)
	Created{
		committable:C,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	Committed{
		path:PathBuf
	}
}

impl<C> Image<C> where C:Committable + 'static {
	pub fn from_obj(obj:DefaultDicomObject) -> Self {Self::Create {obj}}
	pub async fn from_existing<P:AsRef<Path>>(path:P) -> tools::Result<Self> {
		let path = path.as_ref();
		let size = tokio::fs::metadata(path).await.context(format!("getting metadata for {}",path.display()))?.len();
		let (obj, checksum) = checked_load(path).await?;
		Ok(Image::Existing {
			path:path.to_path_buf(),
			size,checksum,obj,
		})
	}
	pub async fn move_existing<P:AsRef<Path>>(org_path:P) -> tools::Result<Self>{
		if let Self::Existing { path, size, checksum, obj } = Self::from_existing(org_path).await?{
			Ok(Self::Move {
				org_path: path,
				size, checksum, obj,
			})
		} else { unreachable!(); }
	}
	pub async fn copy_existing<P:AsRef<Path>>(org_path:P) -> tools::Result<Self>{
		if let Self::Existing { path, size, checksum, obj } = Self::from_existing(org_path).await?{
			Ok(Self::Copy {
				org_path: path,
				size, checksum, obj,
			})
		} else { unreachable!(); }
	}
	pub async fn into_saved(self) -> tools::Result<Self> {
		match self {
			// file needs to be created, write into a committable
			Image::Create { obj } => {
				let path=PathBuf::from(gen_filepath(&obj)?);
				let c_path = complete_filepath(&path);
				let p=c_path.parent().unwrap();
				let obj = Arc::new(obj);
				let obj_shared=obj.clone();
				tokio::fs::create_dir_all(p).await
					.context(format!("Failed creating storage path {}",p.display()))?;

				let (committable,checksum) = spawn_blocking(move || {
					let inner = C::create(path)?;
					let mut checksum = md5::Context::new();
					let mut writer = crate::db::file::Md5Proxy {context:&mut checksum,inner};
					obj_shared.write_all(&mut writer).map_err(|e|DicomError(e.into()))?;
					Ok::<_, tools::Error>((writer.inner,checksum))
				}).await??;
				let size = std::fs::metadata(&c_path)?.len();
				Ok(Self::Created{
					committable,
					checksum:checksum.finalize(),
					size,
					obj:Arc::into_inner(obj).unwrap()
				})
			},
			Image::Move { org_path, size, checksum, obj } => {
				let path=PathBuf::from(gen_filepath(&obj)?);
				let c_path = complete_filepath(&path);
				if c_path != org_path.canonicalize()? { // if file is not already in place, create a copy
					if std::fs::exists(&c_path)?{
						return Err(tools::Error::FileAlreadyExists {path:c_path});
					}
					if let Err(_)=tokio::fs::hard_link(&org_path,&c_path).await { // try hardlink
						tokio::fs::copy(&org_path,&c_path).await?; // fall back to copy
					}
					Ok(Self::Moved {
						org_path:Some(org_path),
						committable: Ok(C::from_existing(path)?),
						size, checksum,obj
					})
				} else {
					// file is literally the same, so just take ownership and *don't* make it a
					// committable, we don't want it to be deleted on an abort
					// Just keep the (relative) path, we're gonna need it
					Ok(Self::Moved {
						org_path:None,
						committable: Err(path),
						size, checksum,
						obj
					})
				}
			},
			Image::Copy { org_path, size, checksum, obj } => {
				let path=PathBuf::from(gen_filepath(&obj)?);
				let c_path = complete_filepath(&path);
				if std::fs::exists(&c_path)?{
					return Err(tools::Error::FileAlreadyExists {path:c_path.to_path_buf()});
				}
				tokio::fs::copy(&org_path,&c_path).await?;
				Ok(Self::Copied {
					committable: C::from_existing(path)?,
					size, checksum, obj
				})
			},
			// file already exists, nothing to do
			Image::Existing {..} | Image::Created {..} | Image::Committed{..} | Image::Moved {..} | Image::Copied {..}
				=> Ok(self),
		}
	}
	pub fn owned(&self) -> bool {
		match self {
			Image::Existing { .. } => false,
			Image::Copy {..} | Image::Move {..} | Image::Create { .. } | Image::Created { .. }
			| Image::Committed {..} | Image::Moved {..} | Image::Copied {..}
				=> true, // @todo check if owned
		}
	}
	pub fn get_fileinfo(&self) -> Option<FileInfo> {
		match self {
			Image::Create { .. } | Image::Committed { .. } | Image::Move { .. } | Image::Copy {..}
				=> None,
			Image::Existing { path, size, checksum, .. }
				=> Some(FileInfo::new(path,checksum.clone(),false, *size)),
			Image::Created { size, checksum, .. }
			| Image::Copied { size, checksum, ..}
			| Image::Moved { size, checksum, .. }
				=> Some(FileInfo::new(self.get_path(),checksum.clone(),true,*size)),
		}
	}
	pub fn get_md5(&self) -> Option<&Digest> {
		match self {
			Image::Create { .. } | Image::Committed { .. } => None,
			Image::Existing { checksum, .. } | Image::Move { checksum, .. }
			| Image::Moved { checksum, .. } | Image::Copy { checksum, .. }
			| Image::Copied { checksum, .. } | Image::Created { checksum, .. }
			 	=> Some(checksum)
		}
	}
	pub fn get_path(&self) -> &Path {
		match self {
			Image::Move { .. } | Image::Create { .. } | Image::Copy { .. } => panic!("File not yet created"),
			Image::Committed { path } | Image::Existing { path, .. } => path,
			Image::Created { committable, .. } | Image::Copied { committable, .. }  => committable.get_targetpath(),
			Image::Moved {committable, ..} =>
				match &committable
				{
					Ok(c) => c.get_targetpath(),
					Err(p)=>p,
				},
		}
	}
	pub fn commit(&mut self) {
		match std::mem::replace(self, Image::Committed { path:self.get_path().to_path_buf() }) {
			Image::Create { .. } => panic!("Trying to commit not yet saved image."),
			Image::Move { .. } => panic!("Trying to commit not yet moved image."),
			Image::Copy { .. } => panic!("Trying to commit not yet copied image."),
			Image::Existing { .. } => (), // file exists already, nothing to be done
			Image::Committed { .. } => (), // file was already committed, nothing to be done
			Image::Created { mut committable, .. } | Image::Copied { mut committable, .. } => {
				committable.commit()
			},
			Image::Moved { org_path, committable, .. } => {
				match committable {
					Ok(mut c) => {
						c.commit();
						if let Some(org_path) = org_path {
							if let Err(e) = std::fs::remove_file(&org_path) {
								warn!("Failed to remove original file {} in a move op ({e})", org_path.display());
							}
						}
					}
					Err(_) => {} // not a Committable, nothing to be done, just keep it
				}
			}
		}
	}
}

impl From<DefaultDicomObject> for Image<StandardFile> {
	fn from(obj:DefaultDicomObject) -> Self {Self::Create {obj}}
}

impl<C> AsRef<DefaultDicomObject> for Image<C> where C:Committable {
	fn as_ref(&self) -> &DefaultDicomObject {
		match self {
			Image::Create {obj, .. }
			| Image::Existing {obj, ..}
			| Image::Move {obj, ..}
			| Image::Copy {obj, ..}
			| Image::Created {obj, ..}
			| Image::Moved {obj,..}
			| Image::Copied {obj, ..} => obj,
			Image::Committed {..} => panic!("Invalid object reference on committed image file"),
		}
	}
}
