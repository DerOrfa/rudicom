use crate::db;
use crate::server::http_error::{HttpError, JsonError, TextError};
use crate::storage::async_store;
use crate::tools::remove::remove;
use crate::tools::store::store;
use crate::tools::verify::verify_entry;
use crate::tools::{get_instance_dicom, lookup_instance_file, Error};
use crate::tools::{Context, Error::DicomError};
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, Query};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Json;
use axum_extra::body::AsyncReadBody;
use dicom::pixeldata::image::ImageFormat;
use dicom::pixeldata::PixelDecoder;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Cursor;
use crate::db::lookup_uid;
use crate::tools::Error::NotFound;

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

async fn del_entry(Path((table,id)):Path<(String, String)>) -> Result<(),JsonError>
{
	let ctx = format!("deleting {table}:{id}");
	let entry = lookup_uid(table, id).await?.ok_or(NotFound).context(ctx)?;
	remove(entry.id()).await.map_err(|e|e.into())
}

async fn verify(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let ctx = format!("verifying {table}:{id}");
	let entry = lookup_uid(table, id).await?.ok_or(NotFound).context(ctx)?;
	let fails:Vec<_> = verify_entry(entry).await?.into_iter()
		.map(|e|if let Error::ChecksumErr { checksum, file } = e
		{
			json!({"checksum_error":{"file":file,"actual_checksum":checksum}})
		}else {
			json!({"error":Value::from(&e)})
		})
		.collect();

	Ok(
		if fails.is_empty() { StatusCode::OK.into_response() }
		else { (StatusCode::INTERNAL_SERVER_ERROR, Json(fails)).into_response() }
	)
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
			let path = format!("/{}/{}",ob.id().table(),ob.id().str_key());
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
	let filename_for_header = format!(r#"attachment; filename="MR.{}.ima""#, id);
	let not_found = format!("Instance {} not found", id);
	if let Some(file)=lookup_instance_file(id).await.context("looking up fileinfo")?
	{
		let file= tokio::fs::File::open(file.get_path()).await?;
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
		Ok((StatusCode::NOT_FOUND, not_found).into_response())
	}
}

#[cfg(feature = "dicom-json")]
async fn get_instance_json_ext(Path(id):Path<String>) -> Result<Response,JsonError>
{
	let err = format!("Instance {id} not found");
	if let Some(obj)=get_instance_dicom(id).await?
	{
		dicom_json::to_value(obj)
			.map(|v|Json(v).into_response())
			.map_err(|e|e.into())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND, err).into_response())
	}
}

#[derive(Deserialize)]
pub(crate) struct ImageSize {
	width: u32,
	height: u32,
}

async fn get_instance_png(Path(id):Path<String>, size: Option<Query<ImageSize>>) -> Result<Response,TextError>
{
	let ctx = format!("decoding pixel data of {id}");
	let not_found = format!("Instance {} not found", id);
	if let Some(obj)=get_instance_dicom(id).await?
	{
		let mut buffer = Cursor::new(Vec::<u8>::new());
		let mut image = obj.decode_pixel_data()
			.and_then(|p|p.to_dynamic_image(0))
			.map_err(|e|DicomError(e.into()))
			.context(ctx)?;
		if let Some(size) = size{
			image=image.thumbnail(size.width,size.height);
		}
		image.write_to(&mut buffer, ImageFormat::Png).expect("Unexpectedly failed to write png data to memory buffer");

		Ok((
			[(header::CONTENT_TYPE, "image/png")],
			buffer.into_inner()
		).into_response())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND, not_found).into_response())
	}
}
