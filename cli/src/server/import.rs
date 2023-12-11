use axum::routing::post;
use axum::extract::Query;
use std::collections::HashMap;
use axum::response::{IntoResponse, Response};
use anyhow::anyhow;
use serde_json::json;
use crate::server::{JsonError, TextError};
use crate::tools;
use futures::StreamExt;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/tools/import/json",post(import_json))
        .route("/tools/import/text",post(import_text))
}

async fn import_text(Query(mut params): Query<HashMap<String, String>>, pattern:String) -> Result<Response,TextError>
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

async fn import_json(Query(mut params): Query<HashMap<String, String>>, pattern:String) -> Result<Response,JsonError>
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

