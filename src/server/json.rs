use std::collections::{BTreeMap, HashMap};
use crate::db;
use crate::server::http_error::{HttpError, IntoHttpError};
use crate::tools::Error::IdNotFound;
use crate::tools::{entries_for_record, Context};
use axum::extract::{FromRequest, Path, Request};
use axum::extract::rejection::{FormRejection, JsonRejection};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use mime::Mime;
use serde_json::Value;
use surrealdb::sql;
use crate::server::lookup_or;
use crate::tools::conv::value_to_json;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/{table}",get(query_table))
        .route("/{table}/{id}",get(query_entry))
		.route("/{table}/{id}/col/{name}",get(get_value).post(set_value).delete(delete_value))
		.route("/{table}/{id}/parents",get(get_entry_parents))
		.route("/{table}/{id}/instances",get(query_instances))
		.route("/{table}/{id}/series",get(query_series))
}

pub fn get_mime(headers: &HeaderMap<HeaderValue>) -> Option<Mime>
{
	headers.get(header::CONTENT_TYPE)
		.and_then(|h| h.to_str().ok())
		.and_then(|h| h.parse::<Mime>().ok())
}
pub fn is_json(mime: &Mime) -> bool {
	mime.type_() == "application" && 
		(mime.subtype() == "json" || mime.suffix().is_some_and(|name| name == "json"))
}

#[derive(Default,Debug)]
struct Content(surrealdb::Value);
enum ContentRejection
{
	JsonReject(JsonRejection),
	FormReject(FormRejection),
	InvalidContent
}

impl IntoResponse for ContentRejection
{
	fn into_response(self) -> Response {
		match self {
			ContentRejection::JsonReject(j) => j.into_response(),
			ContentRejection::FormReject(f) => f.into_response(),
			ContentRejection::InvalidContent => StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response()
		}
	}
}

impl<S> FromRequest<S> for Content where S: Send + Sync
{
	type Rejection = ContentRejection;

	async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection>
	{
		let inner = if get_mime(req.headers()).is_some_and(|m|is_json(&m)){
			let json_val = Json::<Value>::from_request(req, state).await
				.map_err(|e|ContentRejection::JsonReject(e))?;
			crate::tools::conv::json_to_value(json_val.0)
		} else {
			match axum::Form::<HashMap<String,String>>::from_request(req,state).await {
				Ok(form) => {
					let map:BTreeMap<String, sql::Value> =
						form.0.into_iter().map(|(k,v)| (k,v.into()))
							.collect();
					sql::Value::from(map)
				}
				Err(FormRejection::InvalidFormContentType(_)) => Err(ContentRejection::InvalidContent)?,
				Err(e) => Err(ContentRejection::FormReject(e))?,
			}
		};
		Ok(Content(surrealdb::Value::from_inner(inner)))
	}
}

async fn query_instances(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	let instances = entries_for_record(entry.id(),"instances").await.into_http_error(&headers)?;
	Ok(Json(serde_json::Value::from(instances)).into_response())
}
async fn query_series(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	let instances:Vec<_> = entries_for_record(entry.id(),"series").await.into_http_error(&headers)?;
	Ok(Json(serde_json::Value::from(instances)).into_response())
}
async fn query_table(headers: HeaderMap,Path(table):Path<String>) -> Result<Response, HttpError>
{
	let qry = db::list_entries(table).await.into_http_error(&headers)?;
	Ok(Json(serde_json::Value::from(qry)).into_response())
}

async fn query_entry(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	Ok(Json(serde_json::Value::from(entry)).into_response())
}

async fn get_entry_parents(headers: HeaderMap,Path(path):Path<(String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&path).await.into_http_error(&headers)?;
	let mut ret:Vec<_>=vec![];
	let parents = db::find_down_tree(entry.id().clone()).into_http_error(&headers)?;
	for p_id in parents
	{
		let ctx = format!("looking up parent {p_id} of {}:{}",path.0,path.1);
		let e=db::lookup(&p_id).await
			.and_then(|e|e.ok_or(IdNotFound {id:p_id.str_key()}))
			.context(ctx).into_http_error(&headers)?;
		ret.push(e);
	}
	Ok(Json(serde_json::Value::from(ret)).into_response())
}


async fn get_value(headers: HeaderMap,Path((table,uid,name)):Path<(String, String, String)>) -> Result<Response, HttpError>
{
	let path = (table,uid);
	let value = lookup_or(&path).await.into_http_error(&headers)?
		.get(name.as_str()).cloned()
		.ok_or(IdNotFound {id:format!("'{name}' in existing {}:{}",path.0,path.1)})
		.into_http_error(&headers)?;

	Ok((StatusCode::FOUND,Json(value_to_json(value.into_inner()))).into_response())
}

async fn set_value(headers: HeaderMap,Path((table,uid,name)):Path<(String, String, String)>,content:Content) -> Result<Response, HttpError>
{
	let entry = lookup_or(&(table,uid)).await.into_http_error(&headers)?;
	db::set_value(entry.id(),name,content.0).await
		.map(|v|(StatusCode::ACCEPTED,Json(value_to_json(v.into_inner()))).into_response())
		.into_http_error(&headers)
}

async fn delete_value(headers: HeaderMap,Path((table,uid,name)):Path<(String, String, String)>) -> Result<Response, HttpError>
{
	let entry = lookup_or(&(table,uid)).await.into_http_error(&headers)?;
	db::delete_value(entry.id(),name).await
		.map(|v|(StatusCode::ACCEPTED,Json(value_to_json(v.into_inner()))).into_response())
		.into_http_error(&headers)
}