use axum::routing::post;
use axum::extract::Query;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use crate::server::http_error::{HttpError, IntoHttpError};
use futures::StreamExt;
use crate::tools::import::{import_glob, import_glob_as_text, ImportConfig, ImportMode};


pub(super) fn router() -> axum::Router
{
    axum::Router::new()
        .route("/import/json",post(|headers,config,pattern|import_json(headers,config,ImportMode::Import,pattern)))
        .route("/import/text",post(|headers,config,pattern|import_text(headers,config,ImportMode::Import, pattern)))
		.route("/store/json",post(|headers,config,pattern|import_json(headers,config,ImportMode::Store,pattern)))
		.route("/store/text",post(|headers,config,pattern|import_text(headers,config,ImportMode::Store, pattern)))
		.route("/move/json",post(|headers,config,pattern|import_json(headers,config,ImportMode::Move,pattern)))
		.route("/move/text",post(|headers,config,pattern|import_text(headers,config,ImportMode::Move, pattern)))
}

async fn import_text(headers: HeaderMap,Query(config): Query<ImportConfig>, mode:ImportMode, pattern:String) -> Result<Response, HttpError>
{
	let stream= import_glob_as_text(pattern,config,mode).into_http_error(&headers)?
		.map(|r|match r {
			Ok(s) => s+"\n",
			Err(e) => format!("Import task panicked:{e}")
		});
	Ok(axum_streams::StreamBodyAs::text(stream).into_response())
}

async fn import_json(headers: HeaderMap,Query(config): Query<ImportConfig>,mode:ImportMode, pattern:String) -> Result<Response, HttpError>
{
	let stream=import_glob(pattern,config,mode).into_http_error(&headers)?
		.map(|r|match r {
			Ok(s) => serde_json::to_value(s)
					.unwrap_or_else(|e|json!({"error":"serialisation failed","cause":format!("{e}")})),
			Err(e) => json!({"task aborted":format!("{e}")})
		});
	Ok(axum_streams::StreamBodyAs::json_array(stream).into_response())
}

