use axum::routing::post;
use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use crate::server::http_error::{JsonError, TextError};
use crate::tools;
use futures::StreamExt;
use serde::Deserialize;

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/tools/import/json",post(import_json))
        .route("/tools/import/text",post(import_text))
}

#[derive(Deserialize)]
struct ImportConfig {registered:Option<bool>,existing:Option<bool>}

async fn import_text(Query(config): Query<ImportConfig>, pattern:String) -> Result<Response,TextError>
{
	let registered = config.registered.unwrap_or(false);
	let existing = config.existing.unwrap_or(false);
	let stream= tools::import::import_glob_as_text(pattern,registered,existing)?
		.map(|r|match r {
			Ok(s) => s+"\n",
			Err(e) => format!("Import task panicked:{e}")
		});
	Ok(axum_streams::StreamBodyAs::text(stream).into_response())
}

async fn import_json(Query(config): Query<ImportConfig>, pattern:String) -> Result<Response,JsonError>
{
	let registered = config.registered.unwrap_or(false);
	let existing = config.existing.unwrap_or(false);
	let stream=tools::import::import_glob(pattern,registered,existing)?
		.map(|r|match r {
			Ok(s) => serde_json::to_value(s)
					.unwrap_or_else(|e|json!({"error":"serialisation failed","cause":format!("{e}")})),
			Err(e) => json!({"task aborted":format!("{e}")})
		});
	Ok(axum_streams::StreamBodyAs::json_array(stream).into_response())
}

