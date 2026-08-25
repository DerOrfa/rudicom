use std::path::{Path, PathBuf};
use std::io::{ErrorKind, Write};
use tracing::{debug, error, warn};
use crate::tools::complete_filepath;

/// Wrapper around an implementor of [Write] that will be deleted from storage when dropped.
/// Unless it's committed.
pub trait Committable: Sized + Write + Send {
	/// Create the file
	///
	/// Trying to create over an existing file should fail.
	/// If filename is a relative path, file should be created in the configured storage path
	/// (but only the relative path be saved).
	fn create(filename:PathBuf) -> std::io::Result<Self>;
	/// Construct a committable from an existing file.
	/// If filename is a relative path, it should assume to be in the configured storage path
	/// (but only the relative path be saved).
	fn from_existing(filename:PathBuf) -> std::io::Result<Self>;
	fn create_async(filename:PathBuf) -> tokio::task::JoinHandle<std::io::Result<Self>> where Self: Sized + Send + 'static {
		tokio::task::spawn_blocking(move || Self::create(filename))
	}

	/// Commiting to the file.
	///
	/// This must be non-failable
	fn commit(&mut self);
	/// Cancel, and with this remove the file.
	///
	/// This may fail.
	fn cancel(&mut self) -> std::io::Result<()>;

	/// Get intended path for the commited file.
	///
	/// This returns the saved path inside the storage, not the canonical one.
	fn get_targetpath(&self) -> &Path;
}

#[derive(Debug)]
pub struct StandardFile {
	filepath: PathBuf,
	file: std::fs::File,
	committed: bool,
}

impl Committable for StandardFile {
	fn create(filepath:PathBuf) -> std::io::Result<Self> {
		std::fs::File::create_new(complete_filepath(&filepath))
			.map(|file|Self{filepath,file,committed:false})
	}

	fn from_existing(filepath: PathBuf) -> std::io::Result<Self> {
		std::fs::File::open(complete_filepath(&filepath))
			.map(|file|Self{filepath,file,committed:false})
	}

	fn commit(&mut self) {self.committed = true;}

	fn cancel(&mut self) -> std::io::Result<()> {
		let mut filename = complete_filepath(&self.filepath);
		if self.committed {
			error!("Cancelling already committed file");
		} else {
			tokio::spawn(async move {
				match tokio::fs::remove_file(&filename).await {
					Ok(()) => {
						if filename.pop(){// if there is a parent path, try to delete it as far as possible
							crate::tools::remove::remove_path(filename.clone(), &crate::config::get().paths.storage_path).await
						} else { Ok(()) }
					},
					Err(e) => match e.kind() {
						ErrorKind::NotFound => {debug!("Trying to roll back file {}, but its not there??", filename.display());Ok(())}, // that's fine, weird though
						_ => Err(e)
					}
				}.map_err(|e|error!("Error rolling back file {}: {e}",filename.display()))
			});
		}
		Ok(())
	}

	fn get_targetpath(&self) -> &Path { self.filepath.as_path() }
}

impl Drop for StandardFile {
	fn drop(&mut self) {
		if !self.committed && let Err(e) = self.cancel(){
			warn!("Cancelling file {:?} failed ({e})", self.filepath.display());
		}
	}
}

impl Write for StandardFile {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {self.file.write(buf)}
	fn flush(&mut self) -> std::io::Result<()> {self.file.flush()}
}
