use std::path::PathBuf;
use std::io::{ErrorKind, Write};
use tempfile::NamedTempFile;
use tracing::error;
use crate::tools;
use crate::tools::Context;

pub trait Committable: Sized + Write + Send {
	fn create(filename:PathBuf) -> std::io::Result<Self>;
	fn create_async(filename:PathBuf) -> tokio::task::JoinHandle<std::io::Result<Self>> where Self: Sized + Send + 'static {
		tokio::task::spawn_blocking(move || Self::create(filename))
	}
	fn commit(&mut self) -> tools::Result<std::fs::File>;
	fn cancel(self){} //default impl silently drops the file
}

pub struct CompatibleFile<W> {
	un_commited: Option<NamedTempFile<W>>,
	target: PathBuf,
}

impl Committable for CompatibleFile<std::fs::File> {
	fn create(target:PathBuf) -> std::io::Result<Self> {
		NamedTempFile::with_prefix_in("rudicom_tmp_",&crate::config::get().paths.storage_path)
			.map(|t|Self{un_commited: Some(t),target})
	}

	fn commit(&mut self) -> tools::Result<std::fs::File> {
		if let Some(mut tmp) = self.un_commited.take() {
			tmp.flush()?;
			tmp.persist(self.target.as_path()).map_err(|p|p.error)
				.context(format!("Failed to commit {}", self.target.display()))
		} else {
			Err(tools::Error::FileAlreadyCommited {path: self.target.clone()})
		}
	}

	fn cancel(mut self) {
		if let Some(_) = self.un_commited.take() {
			// just drop it, that's literally what it's made for
		} else {
			error!("Cancelling already committed file")
		}
	}
}

impl<W> Write for CompatibleFile<W> where W:Write {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		match &mut self.un_commited {
			Some(f) => f.write(buf),
			None => Err(std::io::Error::new(ErrorKind::AlreadyExists,"File already commited")),
		}
	}

	fn flush(&mut self) -> std::io::Result<()> {
		match &mut self.un_commited {
			Some(f) => f.flush(),
			None => Err(std::io::Error::new(ErrorKind::AlreadyExists,"File already commited")),
		}
	}
}
