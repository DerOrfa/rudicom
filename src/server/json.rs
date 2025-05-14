use crate::db;
use crate::server::http_error::JsonError;
use crate::tools::Error::IdNotFound;
use crate::tools::{entries_for_record, Context};
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use crate::server::lookup_or;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/{table}",get(query_table))
        .route("/{table}/{id}",get(query_entry))
		.route("/{table}/{id}/parents",get(get_entry_parents))
		.route("/{table}/{id}/instances",get(query_instances))
		.route("/{table}/{id}/series",get(query_series))
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
