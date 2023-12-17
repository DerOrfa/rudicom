use axum::{Json, Router};
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing;
use crate::{config, db};

#[cfg(feature = "html")]
mod html;
mod json;
mod import;
mod other;

pub(crate) struct TextError(anyhow::Error);
impl IntoResponse for TextError {
	fn into_response(self) -> Response
	{
		tracing::error!("Internal error {} reported (root cause {})",self.0,self.0.root_cause());
		(
			StatusCode::INTERNAL_SERVER_ERROR,
			format!("{:#}", self.0)
		).into_response()
	}
}
impl<E> From<E> for TextError where E: Into<anyhow::Error>,
{
	fn from(err: E) -> Self {Self(err.into())}
}

pub(crate) struct JsonError(anyhow::Error);
impl IntoResponse for JsonError {
	fn into_response(self) -> Response
	{
		let chain:Vec<_>=self.0.chain().into_iter()
			.map(|e|e.to_string())
			.collect();
		tracing::error!("Internal error {} reported (root cause {})",self.0,self.0.root_cause());
		(StatusCode::INTERNAL_SERVER_ERROR,Json(chain)).into_response()
	}
}
impl<E> From<E> for JsonError where E: Into<anyhow::Error>,
{
	fn from(err: E) -> Self {Self(err.into())}
}

#[derive(Serialize,Clone)]
struct Info
{
	version:String,
	db_version:String,
	storage_path:String
}

async fn server_info() -> Info
{
	Info{
		version:format!("{} v{}",env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
		db_version:db::version().await.unwrap(),
		storage_path:config::get::<String>("storage_path").unwrap(),
	}
}
pub async fn serve(listener:TcpListener) -> anyhow::Result<()>
{
	let inf=server_info().await;
	tracing_subscriber::registry()
		.with(
			tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "rudicom=debug".into()),
		)
		.with(tracing_subscriber::fmt::layer())
		.init();

	tracing::info!("listening on {}", listener.local_addr()?);
	tracing::info!("database is {}",inf.db_version);
	tracing::info!("storage path is {}",inf.storage_path);

	// build our application with a route
	let mut app = Router::new();
	app = app
		.route("/info/json",get(||async {Json(inf)}))
		.merge(other::router())
		.merge(json::router())
		.merge(import::router())
		.layer(DefaultBodyLimit::max(
			config::get::<usize>("upload_sizelimit_mb").unwrap_or(10)*1024*1024
		))
		;
	#[cfg(feature = "html")]
	{
		app = app.merge(html::router());
	}

	// run it
	axum::serve(listener,app.into_make_service()).await.map_err(|e|e.into())
}
