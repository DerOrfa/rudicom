use axum::routing::get;
use axum::Json;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use surrealdb::sql;
use crate::db;
use crate::db::Selector;
use crate::server::http_error::JsonError;
use crate::tools::{Context, Error};

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/studies/json",get(get_studies))
        .route("/:table/:id/json",get(get_entry))
		.route("/:table/:id/parents",get(get_entry_parents))
		.route("/:table/:id/json/*query",get(query))
}

async fn get_studies() -> Result<Json<Vec<serde_json::Value>>,JsonError>
{
	let studies:Vec<_> = db::list_entries("studies").await?.into_iter()
		.map(serde_json::Value::from).collect();
	Ok(Json(studies))
}

async fn get_entry(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	db::lookup(&id).await?
		.ok_or(Error::IdNotFound {id}.into())
		.map(|e|Json(serde_json::Value::from(e)).into_response())
}

async fn query(Path((table,id,query)):Path<(String, String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	// @todo that lookup is only needed to trigger the NotFound
	let e = db::lookup(&id).await?
		.ok_or(Error::IdNotFound {id:id.clone()}).context(format!("Looking for {query} in {id}",))?;

	let query=query.replace("/",".");
	db::list_json(e.id(),Selector::All,query).await
		.map(|v|Json(v).into_response())
		.map_err(Error::into)
}

async fn get_entry_parents(Path((table,id)):Path<(String, String)>) -> Result<Response,JsonError>
{
	let id = sql::Thing::from((table, id));
	let mut ret:Vec<serde_json::Value>=Vec::new();
	let parents = db::find_down_tree(&id).await?;

	if parents.is_empty() {	return Err(Error::NotFound.context(format!("no parents for {id} found")).into()) }
	for p_id in parents
	{
		let e=db::lookup(&p_id).await.transpose()
			.ok_or(Error::NotFound)
			.and_then(|r|r.map(serde_json::Value::from))
			.context(format!("looking up parent {p_id} of {id}"))?;
		ret.push(e);//@todo this results in split up id. Maybe fix that
	}
	Ok(Json(ret).into_response())
}
