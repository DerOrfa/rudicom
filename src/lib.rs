#![recursion_limit = "512"]

pub use surrealdb::sql::Value as DbVal;
pub use anyhow::Result;

pub mod db;
pub mod dcm;
pub mod storage;
pub mod config;
pub mod tools;
pub mod server;

use dcm::{extract,get_attr_list};
use crate::db::Entry;

