use axum::{Json, Router};
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::signal;
use tracing;
use crate::{config, db};
use crate::db::DB;
use crate::server::http_error::TextError;
use crate::tools::Error::IdNotFound;
use crate::tools::Result;

#[cfg(feature = "html")]
mod html;
mod json;
mod import;
mod other;
mod http_error;

#[derive(Serialize,Clone)]
pub struct Info
{
	version:String,
	pub db_version:String,
	pub storage_path:String
}

pub async fn server_info() -> Info
{
	Info{
		version:format!("{} v{}",env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
		db_version:format!("{}",DB.version().await.unwrap()),
		storage_path:config::get().paths.storage_path.to_string_lossy().into(),
	}
}

pub async fn backup() -> std::result::Result<Response, TextError>
{
	let export = DB.export(()).await?;
	Ok(Body::from_stream(export).into_response())
}

pub async fn serve(listener:TcpListener) -> Result<()>
{
	let inf=server_info().await;
	tracing::info!("listening on http://{}", listener.local_addr()?);

	// build our application with a route
	let mut app = Router::new();
	app = app
		.nest("/api", other::router()
			.route("/info", get(||async {Json(inf)}))
			.merge(json::router())
		)
		.nest("/tools",import::router()
			.route("/backup", get(backup))
		)
		.layer(DefaultBodyLimit::max(config::get().limits.upload_sizelimit.as_u64() as usize))
		;
	#[cfg(feature = "html")]
	{
		app = app.nest("/html", html::router());
	}

	// run it
	axum::serve(listener,app.into_make_service())
		.with_graceful_shutdown(shutdown_signal())
		.await.map_err(|e|e.into())
}

async fn shutdown_signal() {
	let ctrl_c = async {
		signal::ctrl_c()
			.await
			.expect("failed to install Ctrl+C handler");
		eprintln!("Got CTRL+C trying graceful shutdown");
	};

	#[cfg(unix)]
		let terminate = async {
		signal::unix::signal(signal::unix::SignalKind::terminate())
			.expect("failed to install signal handler")
			.recv()
			.await;
		eprintln!("Got CTRL+C trying graceful shutdown");
	};

	#[cfg(not(unix))]
		let terminate = std::future::pending::<()>();

	tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

pub async fn lookup_or(rec:&(String, String)) -> crate::tools::Result<db::Entry>
{
	db::lookup_uid(rec.0.as_str(), rec.1.clone()).await?.ok_or(IdNotFound {id:rec.1.clone()})
}

