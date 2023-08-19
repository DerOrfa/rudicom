use std::collections::HashMap;
use std::io::Cursor;
use std::mem::swap;
use axum::response::{IntoResponse, Response, Json, Html};
use axum::http::{header, StatusCode};
use axum::extract::{Path, Query, rejection::BytesRejection};
use axum_extra::body::AsyncReadBody;
use anyhow::{anyhow, Context};
use axum::body::Bytes;
use dicom::pixeldata::PixelDecoder;
use dicom_pixeldata::image::ImageOutputFormat;
use crate::db::{json_id_cleanup, query_for_entry};
use super::{JsonError, TextError};
use crate::tools::{get_instance_dicom, lookup_instance_filepath, store};
use crate::{JsonVal, tools};
use crate::storage::async_store;
use futures::StreamExt;
use html::root::Body;
use itertools::Itertools;
use serde_json::json;
use crate::server::html::{make_table, wrap_body};
use crate::config;

pub(crate) async fn get_studies() -> Result<Json<JsonVal>,JsonError>
{
	let mut studies = crate::db::list("studies").await?;
	for study in &mut studies{
		let series=study.get_mut("series").unwrap();
		let mut new_series= json_id_cleanup(series)?;
		swap(series,&mut new_series);
	}
	Ok(Json(JsonVal::from(studies)))
}

pub(crate) async fn get_studies_html() -> Result<Html<String>,TextError>
{
	let keys=["StudyDate", "StudyTime"].into_iter().map(|s|s.to_string())
		.chain(config::get::<Vec<String>>("study_tags").unwrap().into_iter())//get the rest from the config
		.unique()//make sure there are no duplicates
		.collect();
	let table= make_table(crate::db::list("studies").await?,"Study".to_string(), keys).await
		.map_err(|e|e.context("Failed generating the table"))?;

	let mut builder = Body::builder();
	builder.heading_1(|h|h.text("Studies"));
	builder.push(table);
	Ok(Html(wrap_body(builder.build(), "Studies").to_string()))
}

pub(super) async fn get_instance(Path(id):Path<String>) -> Result<Json<JsonVal>,JsonError>
{
	query_for_entry(("instances",id.as_str()).into()).await
		.map(|v|Json(v)).map_err(|e|e.into())
}

pub(super) async fn store_instance(payload:Result<Bytes,BytesRejection>) -> Result<Response,JsonError> {
	let bytes = payload?;
	if bytes.is_empty(){return Err(anyhow!("Ignoring empty upload").into())}
	let mut md5= md5::Context::new();
	let skip = if bytes.len() >= 132 && &bytes[128..132] == b"DICM" {Some(128)} else { None };
	let obj= async_store::read(bytes, skip,Some(&mut md5))?;
	match store(obj,md5.compute()).await? {
		JsonVal::Null => Ok((StatusCode::CREATED).into_response()),
		JsonVal::Object(ob) => Ok((StatusCode::FOUND,Json(ob)).into_response()),
		_ => Err(anyhow!("Unexpected reply from the database").into())
	}
}

pub(super) async fn get_instance_file(Path(id):Path<String>) -> Result<Response,JsonError> {
	if let Some(path)=lookup_instance_filepath(id.as_str()).await.context("looking up filepath")?
	{
		let file= tokio::fs::File::open(&path).await?;
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

pub(super) async fn get_instance_json(Path(id):Path<String>) -> Result<Response,JsonError>
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

pub(super) async fn get_instance_png(Path(id):Path<String>) -> Result<Response,TextError>
{
	if let Some(obj)=get_instance_dicom(id.as_str()).await?
	{
		let mut buffer = Cursor::new(Vec::<u8>::new());
		obj.decode_pixel_data()?.to_dynamic_image(0)?
			.write_to(&mut buffer, ImageOutputFormat::Png)?;

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

pub(super) async fn import_text(Query(mut params): Query<HashMap<String, String>>, pattern:String) -> Result<Response,TextError>
{
	let registered= params.remove("registered").map_or(Ok(false),|s|s.parse::<bool>())?;
	let existing= params.remove("existing").map_or(Ok(true),|s|s.parse::<bool>())?;
	if !params.is_empty(){
		return Err(anyhow!(r#"unrecognized query parameters ("registered" and "existing" are allowed)"#).into());
	}
	let stream=tools::import::import_glob_as_text(pattern,registered,existing)?
		.map(|r|match r {
			Ok(s) => s+"\n",
			Err(e) => format!("Import task panicked:{e}")
		});
	Ok(axum_streams::StreamBodyAs::text(stream).into_response())
}

pub(super) async fn import_json(Query(mut params): Query<HashMap<String, String>>,pattern:String) -> Result<Response,JsonError>
{
	let registered= params.remove("registered").map_or(Ok(false),|s|s.parse::<bool>())?;
	let existing= params.remove("existing").map_or(Ok(true),|s|s.parse::<bool>())?;
	if !params.is_empty(){
		return Err(anyhow!(r#"unrecognized query parameters ("registered" and "existing" are allowed)"#).into());
	}

	let stream=tools::import::import_glob(pattern,registered,existing)?
		.map(|r|match r {
			Ok(s) => serde_json::to_value(s)
					.unwrap_or_else(|e|json!({"error":"serialisation failed","cause":format!("{e}")})),
			Err(e) => json!({"task aborted":format!("{e}")})
		});
	Ok(axum_streams::StreamBodyAs::json_array(stream).into_response())
}
