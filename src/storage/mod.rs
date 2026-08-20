use crate::storage::file::StandardFile;

pub mod async_store;
mod file;
mod image;

pub type Image<C= StandardFile> = image::Image<C>;
