use std::collections::HashMap;
use std::io::Cursor;
use axum::response::{IntoResponse, Response, Json};
use axum::http::{header, StatusCode};
use axum::extract::{Path, Query, rejection::BytesRejection};
use axum_extra::body::AsyncReadBody;
use anyhow::{anyhow, Context};
use axum::body::Bytes;
use dicom::pixeldata::PixelDecoder;
use dicom_pixeldata::image::ImageOutputFormat;
use super::{JsonError, TextError};
use crate::tools::{get_instance_dicom, lookup_instance_filepath, remove, store};
use crate::{db, JsonVal, tools};
use crate::storage::async_store;
use futures::StreamExt;

#[cfg(feature = "html")]
use html::{root::Body,tables::builders::TableCellBuilder};
#[cfg(feature = "html")]
use crate::server::html::{HtmlEntry,make_entry_page, make_table_from_objects, wrap_body};
#[cfg(feature = "html")]
use tokio::task::JoinSet;
#[cfg(feature = "html")]
use itertools::Itertools;
use serde::Deserialize;
use serde_json::json;
use surrealdb::sql;
use crate::db::Entry;

pub(crate) async fn get_studies() -> Result<Json<Vec<Entry>>,JsonError>
{
	let studies = db::list_table("studies").await?;
	Ok(Json(studies))
}

#[cfg(feature = "html")]
#[derive(Deserialize)]
pub(crate) struct StudyFilter {
	filter: String,
}

#[cfg(feature = "html")]
pub(crate) async fn get_studies_html(filter: Option<Query<StudyFilter>>) -> Result<axum::response::Html<String>,TextError>
{
	let keys=["StudyDate", "StudyTime"].into_iter().map(|s|s.to_string())
		.chain(crate::config::get::<Vec<String>>("study_tags").unwrap().into_iter())//get the rest from the config
		.unique()//make sure there are no duplicates
		.collect();

	let mut studies = db::list_table("studies").await?;
	if let Some(filter) = filter
	{
		studies.retain(|e|e.get_name().find(filter.filter.as_str()).is_some());
	}

	// count instances before as db::list_children cant be used in a closure / also parallelization
	let mut counts=JoinSet::new();
	for id in studies.iter().map(|e|e.id().clone())
	{
		counts.spawn(async move {
			if let Ok(instances)=db::list_values(&id, "series.instances").await
			{
				let instances= instances.into_iter()
					.filter_map(|v|if let sql::Value::Array(array) = v{Some(array)} else {None})
					.flatten().count();
				(id,instances)
			} else { (id, 0) }
		});
	}
	// collect results from above
	let mut instance_count : HashMap<_,_> = HashMap::new();
	while let Some(res) = counts.join_next().await
	{
		let (k,v) = res?;
		instance_count.insert(k,v);
	}
	let countinstances = |obj:&HtmlEntry,cell:&mut TableCellBuilder| {
		let inst_cnt=instance_count[obj.id()];
		cell.text(inst_cnt.to_string());
	};

	let table= make_table_from_objects(
		studies, "Study".to_string(), keys,
		vec![("Instances",countinstances)]
	).await.map_err(|e|e.context("Failed generating the table"))?;

	let mut builder = Body::builder();
	builder.heading_1(|h|h.text("Studies"));
	builder.push(table);
	Ok(axum::response::Html(wrap_body(builder.build(), "Studies").to_string()))
}
#[cfg(feature = "html")]
pub(crate) async fn get_entry_html(Path((table,id)):Path<(String,String)>) -> Result<Response,TextError>
{
	if let Some(entry) = db::lookup((table.as_str(), id.as_str()).into()).await?
	{
		let page = make_entry_page(HtmlEntry(entry)).await?;
		Ok(axum::response::Html(page.to_string()).into_response())
	}
	else
	{
		Ok((StatusCode::NOT_FOUND,Json(json!({"Status":"not found"}))).into_response())
	}
}

pub(super) async fn get_entry(Path((table,id)):Path<(String,String)>) -> Result<Response,JsonError>
{
	if let Some(res)=db::lookup((table.as_str(), id.as_str()).into()).await?
	{
		Ok(Json(res).into_response())
	} else {
		Ok((
			StatusCode::NOT_FOUND,
			Json(json!({"Status":"not found"}))
		).into_response())
	}
}
pub(super) async fn query(Path((table,id,query)):Path<(String,String,String)>) -> Result<Response,JsonError>
{
	if let Some(res) = db::lookup((table.as_str(), id.as_str()).into()).await?
	{
		let query=query.replace("/",".");
		let children=db::list_children(res.id(),query).await?;
		Ok(Json(children).into_response())
	} else {
		Ok((
			StatusCode::NOT_FOUND,
			Json(json!({"Status":"not found"}))
		).into_response())
	}
}
pub(super) async fn del_entry(Path((table,id)):Path<(String,String)>) -> Result<(),JsonError>
{
	remove((table.as_str(),id.as_str()).into()).await.map_err(|e|e.into())
}

pub(super) async fn get_entry_parents(Path((table,id)):Path<(String,String)>) -> Result<Json<Vec<Entry>>,JsonError>
{
	let mut ret:Vec<Entry>=Vec::new();
	for id in db::find_down_tree(&sql::Thing::from((table,id))).await?
	{
		let e=db::lookup(id.clone()).await?
			.expect(format!("lookup for parent {id} not found").as_str());
		ret.push(e);
	}
	Ok(Json(ret))
}

pub(super) async fn store_instance(payload:Result<Bytes,BytesRejection>) -> Result<Response,JsonError> {
	let bytes = payload?;
	if bytes.is_empty(){return Err(anyhow!("Ignoring empty upload").into())}
	let mut md5= md5::Context::new();
	let skip = if bytes.len() >= 132 && &bytes[128..132] == b"DICM" {Some(128)} else { None };
	let obj= async_store::read(bytes, skip,Some(&mut md5))?;
	match store(obj,md5.compute()).await? {
		JsonVal::Null => Ok((
			StatusCode::CREATED,
			Json(json!({"Status":"Success"}))
		).into_response()),
		JsonVal::Object(ob) => {
			let id = ob.get("id").unwrap().as_object().unwrap();
			let path = format!("/instances/{}",id.get("id").unwrap().get("String").unwrap().as_str().unwrap());
			Ok((
				StatusCode::FOUND,
				Json(json!({
					"Status":"AlreadyStored",
					"Path":path,
					"AlreadyStored":ob
				}))
			).into_response())
		},
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

pub(super) async fn get_instance_json_ext(Path(id):Path<String>) -> Result<Response,JsonError>
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

pub(super) async fn get_instance_png(Path(id):Path<String>, size: Option<Query<ImageSize>>) -> Result<Response,TextError>
{
	if let Some(obj)=get_instance_dicom(id.as_str()).await?
	{
		let mut buffer = Cursor::new(Vec::<u8>::new());
		let mut image = obj.decode_pixel_data()?.to_dynamic_image(0)?;
		if let Some(size) = size{
			image=image.thumbnail(size.width,size.height);
		}
		image.write_to(&mut buffer, ImageOutputFormat::Png)?;

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
