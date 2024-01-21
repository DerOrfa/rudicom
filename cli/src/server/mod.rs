use axum::{Json, Router};
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing;
use crate::{config, db};
use crate::tools::Result;

#[cfg(feature = "html")]
mod html;
mod json;
mod import;
mod other;
mod http_error;

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
pub async fn serve(listener:TcpListener) -> Result<()>
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
