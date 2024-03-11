use axum::routing::{delete, get, post};
use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use std::io::Cursor;
use dicom_pixeldata::image::ImageOutputFormat;
use axum::http::{header, StatusCode};
use axum::Json;
use axum_extra::body::AsyncReadBody;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use serde_json::json;
use serde::Deserialize;
use dicom_pixeldata::PixelDecoder;
use crate::db;
use crate::server::http_error::{HttpError, JsonError, TextError};
use crate::storage::async_store;
use crate::tools::{get_instance_dicom, lookup_instance_file, remove};
use crate::tools::store::store;
use crate::tools::verify::verify_entry;
use crate::tools::{Context,Error::DicomError};

pub(super) fn router() -> axum::Router
{
    let mut rtr= axum::Router::new();
	rtr=rtr
		.route("/statistics", get(get_statistics))
        .route("/instances",post(store_instance))
        .route("/:table/:id",delete(del_entry))
		.route("/:table/:id/verify",get(verify))
        .route("/instances/:id/file",get(get_instance_file))
        .route("/instances/:id/png",get(get_instance_png));
    #[cfg(feature="dicom-json")]
    {
        rtr = rtr.route("/instances/:id/json-ext", get(get_instance_json_ext));
    }
    rtr
}

async fn get_statistics() -> Result<Json<db::Stats>,JsonError>
{
	db::statistics().await.map(Json).map_err(|e|e.into())
}

async fn del_entry(Path(id):Path<(String, String)>) -> Result<(),JsonError>
{
	remove(id.into()).await.map_err(|e|e.into())
}

async fn verify(Path(id):Path<(String, String)>) -> Result<Json<Vec<db::File>>,JsonError>
{
	Ok(Json(verify_entry(id.into()).await?))
}

async fn store_instance(payload:Result<Bytes,BytesRejection>) -> Result<Response,JsonError> {
	let bytes = payload.map_err(|e|HttpError::BadRequest {message:format!("failed to receive data {e}")})?;
	if bytes.is_empty(){return Err(HttpError::BadRequest {message:"Ignoring empty upload".into()}.into())}
	let obj= async_store::read(bytes)?;
	match store(obj).await? {
		None => Ok((
			StatusCode::CREATED,
			Json(json!({"Status":"Success"}))
		).into_response()),
		Some(ob) => {
			let path = format!("/{}/{}",ob.id().tb,ob.id().id.to_raw());
			let ob = serde_json::Value::from(ob);
			Ok((
				StatusCode::FOUND,
				Json(json!({
					"Status":"AlreadyStored",
					"Path":path,
					"AlreadyStored":ob
				}))
			).into_response())
		},
	}
}

async fn get_instance_file(Path(id):Path<String>) -> Result<Response,JsonError> {
	if let Some(file)=lookup_instance_file(id.as_str()).await.context("looking up fileinfo")?
	{
		let file= tokio::fs::File::open(file.get_path()).await?;
		let filename_for_header = format!(r#"attachment; filename="MR.{}.ima""#, id);
		Ok((
			StatusCode::OK,
			[
				(header::CONTENT_TYPE, "application/dicom"),
				(header::CONTENT_DISPOSITION, filename_for_header.as_str())
			],
			AsyncReadBody::new(file)
		).into_response())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND, format!("Instance {} not found", id)).into_response())
	}
}

#[cfg(feature = "dicom-json")]
async fn get_instance_json_ext(Path(id):Path<String>) -> Result<Response,JsonError>
{
	if let Some(obj)=get_instance_dicom(id.as_str()).await?
	{
		dicom_json::to_value(obj)
			.map(|v|Json(v).into_response())
			.map_err(|e|e.into())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND, format!("Instance {} not found", id)).into_response())
	}
}

#[derive(Deserialize)]
pub(crate) struct ImageSize {
	width: u32,
	height: u32,
}

async fn get_instance_png(Path(id):Path<String>, size: Option<Query<ImageSize>>) -> Result<Response,TextError>
{
	if let Some(obj)=get_instance_dicom(id.as_str()).await?
	{
		let mut buffer = Cursor::new(Vec::<u8>::new());
		let mut image = obj.decode_pixel_data()
			.and_then(|p|p.to_dynamic_image(0))
			.map_err(|e|DicomError(e.into()))
			.context(format!("decoding pixel data of {id}"))?;
		if let Some(size) = size{
			image=image.thumbnail(size.width,size.height);
		}
		image.write_to(&mut buffer, ImageOutputFormat::Png).expect("Unexpectedly failed to write png data to memory buffer");

		Ok((
			[(header::CONTENT_TYPE, "image/png")],
			buffer.into_inner()
		).into_response())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND, format!("Instance {} not found", id)).into_response())
	}
}
