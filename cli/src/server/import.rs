use axum::routing::post;
use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use crate::server::http_error::{JsonError, TextError};
use futures::StreamExt;
use crate::tools::import::{import_glob,import_glob_as_text,ImportConfig};

pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/import/json",post(import_json))
        .route("/import/text",post(import_text))
}

async fn import_text(Query(config): Query<ImportConfig>, pattern:String) -> Result<Response,TextError>
{
	let stream= import_glob_as_text(pattern,config)?
		.map(|r|match r {
			Ok(s) => s+"\n",
			Err(e) => format!("Import task panicked:{e}")
		});
	Ok(axum_streams::StreamBodyAs::text(stream).into_response())
}

async fn import_json(Query(config): Query<ImportConfig>, pattern:String) -> Result<Response,JsonError>
{
	let stream=import_glob(pattern,config)?
		.map(|r|match r {
			Ok(s) => serde_json::to_value(s)
					.unwrap_or_else(|e|json!({"error":"serialisation failed","cause":format!("{e}")})),
			Err(e) => json!({"task aborted":format!("{e}")})
		});
	Ok(axum_streams::StreamBodyAs::json_array(stream).into_response())
}

