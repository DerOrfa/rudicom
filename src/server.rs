use axum::{Json, Router, routing::{get, post}};
use std::net::SocketAddr;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing;
use crate::{config, db};

mod handler;

pub(crate) struct TextError(anyhow::Error);
impl IntoResponse for TextError {
	fn into_response(self) -> Response
	{
		tracing::error!("Internal error {} reportet (root cause {})",self.0,self.0.root_cause());
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

pub async fn serve(at:SocketAddr) -> anyhow::Result<()>{
	tracing_subscriber::registry()
		.with(
			tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "rudicom=debug".into()),
		)
		.with(tracing_subscriber::fmt::layer())
		.init();

	// build our application with a route
	let app = Router::new()
		.route("/instances",post(handler::store_instance))
		.route("/instances/:id",get(handler::get_instance))
		.route("/instances/:id/json",get(handler::get_instance_json))
		.route("/instances/:id/file",get(handler::get_instance_file))
		.route("/instances/:id/png",get(handler::get_instance_png))
		;

	// run it
	tracing::info!("listening on {}", at);
	tracing::info!("database is {}",db::version().await?);
	tracing::info!("storage path is {}",config::get::<String>("storage_path")?);
	axum::Server::bind(&at)
		.serve(app.into_make_service())
		.await
		.unwrap();
	Ok(())
}
