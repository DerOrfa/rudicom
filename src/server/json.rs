use axum::routing::get;
use axum::Json;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use surrealdb::sql;
use axum::http::StatusCode;
use serde_json::json;
use crate::db;
use crate::db::Entry;
use crate::server::JsonError;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/studies/json",get(get_studies))
        .route("/:table/:id/json",get(get_entry))
		.route("/:table/:id/parents",get(get_entry_parents))
		.route("/:table/:id/json/*query",get(query))
}

async fn get_studies() -> Result<Json<Vec<Entry>>,JsonError>
{
	let studies = db::list_table("studies").await?;
	Ok(Json(studies))
}

async fn get_entry(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	if let Some(res)=db::lookup(&id).await?
	{
		Ok(Json(res).into_response())
	} else {
		Ok((
			StatusCode::NOT_FOUND,
			Json(json!({"Status":"not found"}))
		).into_response())
	}
}

async fn query(Path((table,id,query)):Path<(String, String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	if let Some(res) = db::lookup(&id).await?
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

async fn get_entry_parents(Path((table,id)):Path<(String, String)>) -> Result<Json<Vec<Entry>>,JsonError>
{
	let mut ret:Vec<Entry>=Vec::new();
	for id in db::find_down_tree(&sql::Thing::from((table,id))).await?
	{
		let e=db::lookup(&id).await?
			.expect(format!("lookup for parent {id} not found").as_str());
		ret.push(e);
	}
	Ok(Json(ret))
}
