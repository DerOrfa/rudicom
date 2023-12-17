use axum::routing::get;
use axum::Json;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use surrealdb::sql;
use axum::http::StatusCode;
use serde_json::json;
use crate::db;
use crate::server::JsonError;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/studies/json",get(get_studies))
        .route("/:table/:id/json",get(get_entry))
		.route("/:table/:id/parents",get(get_entry_parents))
		.route("/:table/:id/json/*query",get(query))
}

fn not_found() -> Result<Response,JsonError> { Ok((StatusCode::NOT_FOUND,Json(json!({"Status":"not found"}))).into_response()) }

async fn get_studies() -> Result<Json<Vec<serde_json::Value>>,JsonError>
{
	let studies:Vec<_> = db::list_table("studies").await?.into_iter()
		.map(serde_json::Value::from).collect();
	Ok(Json(studies))
}

async fn get_entry(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	if let Some(res)=db::lookup(&id).await?
	{
		Ok(Json(serde_json::Value::from(res)).into_response())
	} else {
		not_found()
	}
}

async fn query(Path((table,id,query)):Path<(String, String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	if let Some(res) = db::lookup(&id).await?
	{
		let query=query.replace("/",".");
		let values=db::list_json(res.id(),query).await?;
		Ok(Json(values).into_response())
	} else {
		not_found()
	}
}

async fn get_entry_parents(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let mut ret:Vec<serde_json::Value>=Vec::new();
	let parents = db::find_down_tree(&sql::Thing::from((table,id))).await?;
	if parents.is_empty() {	return not_found() }
	for id in parents
	{
		let e=db::lookup(&id).await?
			.expect(format!("lookup for parent {id} not found").as_str());
		ret.push(e.into());
	}
	Ok(Json(ret).into_response())
}
