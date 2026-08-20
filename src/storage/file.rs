use std::path::{Path, PathBuf};
use std::io::{ErrorKind, Write};
use tracing::warn;

pub trait Committable: Sized + Write + Send {
	/// Create the file, trying to create an existing file should fail
	fn create(filename:PathBuf) -> std::io::Result<Self>;
	fn create_async(filename:PathBuf) -> tokio::task::JoinHandle<std::io::Result<Self>> where Self: Sized + Send + 'static {
		tokio::task::spawn_blocking(move || Self::create(filename))
	}

	/// commiting to the file
	/// This must be non-failable
	fn commit(&mut self);
	/// This may fail
	fn cancel(&mut self) -> std::io::Result<()>;

	/// Get intended path for the commited file
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
		std::fs::File::create_new(&filepath)
			.map(|file|Self{filepath,file,committed:false})
	}

	fn commit(&mut self) {self.committed = true;}

	fn cancel(&mut self) -> std::io::Result<()> {
		if self.committed {
			warn!("Cancelling already committed file")
		} else if let Err(e) = std::fs::remove_file(&self.filepath) {
			match e.kind() {
				ErrorKind::NotFound => {} // that's fine, weird though
				_ => return Err(e)
			}
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
