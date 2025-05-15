use crate::db;
use crate::db::RegisterResult;
use crate::server::http_error::{HttpError, InnerHttpError, IntoHttpError};
use crate::server::lookup_or;
use crate::storage::async_store;
use crate::tools::remove::remove;
use crate::tools::store::store;
use crate::tools::verify::verify_entry;
use crate::tools::{get_instance_dicom, lookup_instance_file, Error};
use crate::tools::{Context, Error::DicomError};
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Json;
use axum_extra::body::AsyncReadBody;
use axum_extra::extract::OptionalQuery;
use dicom::dictionary_std::tags;
use dicom::pixeldata::image::ImageFormat;
use dicom::pixeldata::PixelDecoder;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Cursor;

pub(super) fn router() -> axum::Router
{
    let mut rtr= axum::Router::new();
	rtr=rtr
		.route("/statistics", get(get_statistics))
        .route("/instances",post(store_instance))
        .route("/{table}/{id}",delete(del_entry))
		.route("/{table}/{id}/verify",get(verify))
		.route("/{table}/{id}/filepath",get(filepath))
        .route("/instances/{id}/file",get(get_instance_file))
        .route("/instances/{id}/png",get(get_instance_png));
    #[cfg(feature="dicom-json")]
    {
        rtr = rtr.route("/instances/{id}/json-ext", get(get_instance_json_ext));
    }
    rtr
}

async fn get_statistics(headers: HeaderMap) -> Result<Json<db::Stats>, HttpError>
{
	db::statistics().await.map(Json).into_http_error(&headers)
}

async fn del_entry(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<(), HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	remove(entry.id()).await.into_http_error(&headers)
}

async fn verify(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	let fails:Vec<_> = verify_entry(entry).await.into_http_error(&headers)?.into_iter()
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

async fn filepath(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	let path = entry.get_path().await.into_http_error(&headers)?;
	Ok(Json(json!({"path":path})).into_response())
}

async fn store_instance(headers: HeaderMap,payload:Result<Bytes,BytesRejection>) -> Result<Response, HttpError> {
	let bytes = payload.map_err(|e|
		HttpError::new(InnerHttpError::BadRequest {message:format!("failed to receive data {e}")}, &headers))?;
	if bytes.is_empty(){
		return Err(HttpError::new(InnerHttpError::BadRequest {message:"Ignoring empty upload".into()}, &headers))
	}
	let obj= async_store::read(bytes).into_http_error(&headers)?;
	match store(obj).await {
		Ok(RegisterResult::Stored(id)) => Ok((StatusCode::CREATED,
			Json(json!({
				"Status":"Success",
				"Path":id.str_path(),
				"ID":id.str_key(),
			}))
		).into_response()),
		Ok(RegisterResult::AlreadyStored(id)) => Ok((StatusCode::FOUND,
			Json(json!({
				"Status":"AlreadyStored",
				"Path":id.str_path(),
				"ID":id.str_key(),
			}))
		).into_response()),
		Err(Error::DataConflict(e)) => {
			Ok((
				StatusCode::CONFLICT,
				Json(json!({
					"Status":"ConflictingMetadata",
					"ExistingData":serde_json::Value::from(e),
				}))
			).into_response())
		}
		Err(Error::Md5Conflict {existing_md5,my_md5, existing_id })  => {
			Ok((
				StatusCode::CONFLICT,
				Json(json!({
					"Status":"ConflictingMd5",
					"Existing Path":existing_id.str_path(),
					"ExistingMd5":existing_md5,
					"ReceivedMd5":my_md5,
				}))
			).into_response())
		}
		Err(e) => Err(HttpError::new(e, &headers))
	}
}

async fn get_instance_file(headers: HeaderMap,Path(id):Path<String>) -> Result<Response, HttpError> 
{
	if let Some(file)=lookup_instance_file(id.clone()).await.into_http_error(&headers)?
	{
		let filename_for_header=file.get_path().file_name()
			.map(|o|o.to_string_lossy().to_string())
			.unwrap_or(format!("Mr.{}.ima",id));
		let file= tokio::fs::File::open(file.get_path()).await.into_http_error(&headers)?;
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
		Err(HttpError::new(Error::IdNotFound {id}, &headers))
	}
}

#[cfg(feature = "dicom-json")]
async fn get_instance_json_ext(headers: HeaderMap,Path(id):Path<String>) -> Result<Response, HttpError>
{
	if let Some(obj)=get_instance_dicom(id.clone()).await.into_http_error(&headers)?
	{
		dicom_json::to_value(obj)
			.map(|v|Json(v).into_response())
			.into_http_error(&headers)
	}
	else
	{
		Err(HttpError::new(Error::IdNotFound {id}, &headers))
	}
}

#[derive(Deserialize)]
pub struct ImageSize {
	width: u32,
	height: u32,
}

async fn get_instance_png(headers: HeaderMap,Path(id):Path<String>, OptionalQuery(size): OptionalQuery<ImageSize>) -> Result<Response, HttpError>
{
	let ctx = format!("decoding pixel data of {id}");
	let not_found = format!("Instance {} not found", id);
	if let Some(obj)=get_instance_dicom(id).await.into_http_error(&headers)?
	{
		if obj.get(tags::PIXEL_DATA).is_none() && obj.get(tags::DOUBLE_FLOAT_PIXEL_DATA).is_none() && obj.get(tags::FLOAT_PIXEL_DATA).is_some() {
			return Ok((StatusCode::NOT_FOUND, not_found).into_response())
		}
		let mut buffer = Cursor::new(Vec::<u8>::new());
		let mut image = obj.decode_pixel_data()
			.and_then(|p|p.to_dynamic_image(0))
			.map_err(|e|DicomError(e.into()))
			.context(ctx).into_http_error(&headers)?;
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
