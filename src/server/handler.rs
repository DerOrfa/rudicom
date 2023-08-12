use std::io::Cursor;
use axum::body::Bytes;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, Value as JsonValue};
use axum::http::{header, StatusCode};
use axum::Json;
use axum::extract::Path;
use axum_extra::body::AsyncReadBody;
use anyhow::Context;
use dicom::pixeldata::PixelDecoder;
use dicom_pixeldata::image::ImageOutputFormat;
use crate::db::query_for_entry;
use crate::storage::async_store;
use super::{JsonError,TextError};
use crate::tools;
use crate::tools::{get_instance_dicom, lookup_instance_filepath};

pub async fn store_instance(bytes:Bytes) -> Response {
	let mut md5=md5::Context::new();
	let obj= async_store::read(bytes,Some(&mut md5)).unwrap();
	match tools::store(obj,md5.compute()).await.unwrap() {
		Value::Null => (StatusCode::CREATED).into_response(),
		Value::Object(ob) => (StatusCode::FOUND,Json(ob)).into_response(),
		_ => (StatusCode::INTERNAL_SERVER_ERROR).into_response()
	}
}

pub(crate) async fn get_instance_file(Path(id):Path<String>) -> Result<Response,JsonError> {
	let path=lookup_instance_filepath(id.as_str()).await.context("looking up filepath")?;
	match tokio::fs::File::open(&path).await
	{
		Ok(file) => {
			let filename_for_header=format!(r#"attachment; filename="MR.{}.ima""#,id);
			Ok((
				StatusCode::OK,
				[
					(header::CONTENT_TYPE, "application/dicom"),
					(header::CONTENT_DISPOSITION, filename_for_header.as_str())
				],
				AsyncReadBody::new(file)
			).into_response())
		}
		Err(e) => {
			Ok((StatusCode::NOT_FOUND, format!("Failed to open file {}: {}", path.to_string_lossy(), e)).into_response())
		}
	}
}

pub(crate) async fn get_instance_json(Path(id):Path<String>) -> Result<Json<JsonValue>,JsonError>
{
	query_for_entry(("instances",id.as_str()).into()).await
		.map(|v|Json(v)).map_err(|e|e.into())
}

pub(crate) async fn get_instance_png(Path(id):Path<String>) -> Result<Response,TextError>
{
	let mut buffer=Cursor::new(Vec::<u8>::new());
	get_instance_dicom(id.as_str()).await?
		.decode_pixel_data()?
		.to_dynamic_image(0)?
		.write_to(&mut buffer,ImageOutputFormat::Png)?;

	Ok((
		[(header::CONTENT_TYPE, "image/png")],
		buffer.into_inner()
	).into_response())
}
