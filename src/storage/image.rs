use std::ffi::CString;
use std::io::ErrorKind;
use crate::dcm::gen_filepath;
use crate::storage::file::{Committable, StandardFile};
use crate::tools;
use crate::tools::Error::DicomError;
use crate::tools::{Context, complete_filepath};
use dicom::object::DefaultDicomObject;
use md5::Digest;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use pyo3::prelude::PyModule;
use pyo3::Python;
use tokio::task::spawn_blocking;
use tracing::warn;
use crate::db::FileInfo;
use crate::storage::checked_load;

/// An object representing an existing or about to be written dicom image file in its various stages.
///
/// Uses [std::fs::File] and [Committable] as backends.
#[derive(Debug)]
pub enum Image<C> where C:Committable,
{
	/// An object ready to be created as a file (always owned)
	Create{
		obj:DefaultDicomObject
	},
	/// An already existing file, its object and parameters (never owned)
	Existing{
		path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// An already existing file, but should be moved.
	///
	/// Considered owned, but only the target file.
	/// Source and target *can* be the same file.
	/// Will delete the source if commited (unless it's the same as target).
	Move {
		org_path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// The saved version of [Image::Move] (owned).
	///
	/// Dropping this will cause a rollback (aka target file will be removed).
	Moved {
		org_path:Option<PathBuf>,
		committable:Result<C,PathBuf>,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// An already existing file, but should be copied (always owned)
	Copy {
		org_path:PathBuf,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// The saved version of [Image::Copy]
	///
	/// Dropping this will cause a rollback (aka file will be removed).
	Copied {
		committable:C,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// The saved version of [Image::Create] (always owned)
	///
	/// Dropping this will cause a rollback (aka file will be removed).
	Created{
		committable:C,
		size:u64,
		checksum:Digest,
		obj:DefaultDicomObject
	},
	/// The commited stage for all
	Committed{
		path:PathBuf
	}
}

impl<C> Image<C> where C:Committable + 'static {
	/// create a [Image::Create] from a [DefaultDicomObject]
	pub fn from_obj(obj:DefaultDicomObject) -> Self {Self::Create {obj}}
	/// create a [Image::Create] from a [DefaultDicomObject]
	pub fn from_obj_filtered(mut obj:DefaultDicomObject) -> tools::Result<Self> {
		if !crate::config::get().filters.is_empty(){
			Python::attach::<_,tools::Result<()>>(|py| {
				for (name,code) in crate::config::get().filters.iter()
					.filter(|(_,code)| !code.is_empty())
				{
					let code = CString::new(code.as_str()).unwrap();
					let name = CString::new(name.as_str()).unwrap();
					let code = PyModule::from_code(py, code.as_ref(), c"", name.as_ref())?;
					tools::filter::filter(code, &mut obj)?;
				}
				Ok(())
			})?;
		}
		Ok(Self::from_obj(obj))
	}
	/// create a [Image::Existing]
	pub async fn from_existing<P:AsRef<Path>>(path:P) -> tools::Result<Self> {
		let path = path.as_ref();
		let size = tokio::fs::metadata(path).await.context(format!("getting metadata for {}",path.display()))?.len();
		let (obj, checksum) = checked_load(path).await?;
		Ok(Image::Existing {
			path:path.to_path_buf(),
			size,checksum,obj,
		})
	}
	/// create a [Image::Move]
	pub async fn move_existing<P:AsRef<Path>>(org_path:P) -> tools::Result<Self>{
		if let Self::Existing { path, size, checksum, obj } = Self::from_existing(org_path).await?{
			Ok(Self::Move {
				org_path: path,
				size, checksum, obj,
			})
		} else { unreachable!(); }
	}
	/// create a [Image::Copy]
	pub async fn copy_existing<P:AsRef<Path>>(org_path:P) -> tools::Result<Self>{
		if let Self::Existing { path, size, checksum, obj } = Self::from_existing(org_path).await?{
			Ok(Self::Copy {
				org_path: path,
				size, checksum, obj,
			})
		} else { unreachable!(); }
	}
	/// Transfer all variants into their saved (but uncommitted) stages.
	/// Will do nothing if the stage is already saved or committed.
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
			// make a cheap copy as [Committable], keep the source for now until commit
			Image::Move { org_path, size, checksum, obj } => {
				let path=PathBuf::from(gen_filepath(&obj)?);
				let c_path = complete_filepath(&path);
				let exists = std::fs::exists(&c_path)?;
				if !exists || c_path.canonicalize()? != org_path.canonicalize()? { // if file is not already in place, create a copy
					if exists {
						Err(std::io::Error::new(ErrorKind::AlreadyExists,format!("{} already exists", c_path.display())))?;
					}
					let p = c_path.parent().unwrap();
					tokio::fs::create_dir_all(p).await
						.context(format!("Failed creating storage path {}",p.display()))?;

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
			// make a plain copy
			Image::Copy { org_path, size, checksum, obj } => {
				let path=PathBuf::from(gen_filepath(&obj)?);
				let c_path = complete_filepath(&path);
				if std::fs::exists(&c_path)?{
					Err(std::io::Error::new(ErrorKind::AlreadyExists,format!("{} already exists", c_path.display())))?;
				}

				let p = c_path.parent().unwrap();
				tokio::fs::create_dir_all(p).await
					.context(format!("Failed creating storage path {}",p.display()))?;

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
	pub fn get_md5(&self) -> Option<String> {
		match self {
			Image::Create { .. } | Image::Committed { .. } => None,
			Image::Existing { checksum, .. } | Image::Move { checksum, .. }
			| Image::Moved { checksum, .. } | Image::Copy { checksum, .. }
			| Image::Copied { checksum, .. } | Image::Created { checksum, .. }
			 	=> Some(format!("{:x}", checksum))
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
