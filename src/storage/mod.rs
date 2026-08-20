use crate::storage::file::CompatibleFile;

pub mod async_store;
mod file;
mod image;

pub type Image<C=CompatibleFile<std::fs::File>> = image::Image<C>;
