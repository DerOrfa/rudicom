use std::collections::{BTreeMap, HashMap};
use crate::db;
use crate::server::http_error::JsonError;
use crate::tools::Error::IdNotFound;
use crate::tools::{entries_for_record, Context};
use axum::extract::{FromRequest, Path, Request};
use axum::extract::rejection::{FormRejection, JsonRejection};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
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

fn json_content_type(headers: &HeaderMap) -> bool {
	let content_type = if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
		content_type
	} else {
		return false;
	};

	let content_type = if let Ok(content_type) = content_type.to_str() {
		content_type
	} else {
		return false;
	};

	let mime = if let Ok(mime) = content_type.parse::<mime::Mime>() {
		mime
	} else {
		return false;
	};

	let is_json_content_type = mime.type_() == "application"
		&& (mime.subtype() == "json" || mime.suffix().is_some_and(|name| name == "json"));

	is_json_content_type
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
		let inner = if json_content_type(req.headers()){
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

async fn query_instances(Path(path):Path<(String, String)>) -> Result<Response,JsonError>
{
	let entry = lookup_or(&path).await?;
	let instances = entries_for_record(entry.id(),"instances").await?;
	Ok(Json(serde_json::Value::from(instances)).into_response())
}
async fn query_series(Path(path):Path<(String, String)>) -> Result<Response,JsonError>
{
	let entry = lookup_or(&path).await?;
	let instances:Vec<_> = entries_for_record(entry.id(),"series").await?;
	Ok(Json(serde_json::Value::from(instances)).into_response())
}
async fn query_table(Path(table):Path<String>) -> Result<Response,JsonError>
{
	let qry = db::list_entries(table).await?;
	Ok(Json(serde_json::Value::from(qry)).into_response())
}

async fn query_entry(Path(path):Path<(String, String)>) -> Result<Response,JsonError>
{
	let entry = lookup_or(&path).await?;
	Ok(Json(serde_json::Value::from(entry)).into_response())
}

async fn get_entry_parents(Path(path):Path<(String, String)>) -> Result<Response,JsonError>
{
	let entry = lookup_or(&path).await?;
	let mut ret:Vec<_>=vec![];
	let parents = db::find_down_tree(entry.id().clone())?;
	for p_id in parents
	{
		let ctx = format!("looking up parent {p_id} of {}:{}",path.0,path.1);
		let e=db::lookup(&p_id).await
			.and_then(|e|e.ok_or(IdNotFound {id:p_id.str_key()}))
			.context(ctx)?;
		ret.push(e);
	}
	Ok(Json(serde_json::Value::from(ret)).into_response())
}


async fn get_value(Path((table,uid,name)):Path<(String, String, String)>) -> Result<Response,JsonError>
{
	let path = (table,uid);
	let value = lookup_or(&path).await?.get(name.as_str()).cloned()
		.ok_or(IdNotFound {id:format!("'{name}' in existing {}:{}",path.0,path.1)})?;

	Ok((StatusCode::FOUND,Json(value_to_json(value.into_inner()))).into_response())
}

async fn set_value(Path((table,uid,name)):Path<(String, String, String)>,content:Content) -> Result<Response,JsonError>
{
	let entry = lookup_or(&(table,uid)).await?;
	db::set_value(entry.id(),name,content.0).await
		.map(|v|(StatusCode::ACCEPTED,Json(value_to_json(v.into_inner()))).into_response())
		.map_err(|e|e.into())
}

async fn delete_value(Path((table,uid,name)):Path<(String, String, String)>) -> Result<Response,JsonError>
{
	let entry = lookup_or(&(table,uid)).await?;
	db::delete_value(entry.id(),name).await
		.map(|v|(StatusCode::ACCEPTED,Json(value_to_json(v.into_inner()))).into_response())
		.map_err(|e|e.into())
}