use axum::{Json, Router, routing::{get, post, delete}};
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tracing;
use crate::{config, db};

mod handler;
#[cfg(feature = "html")]
pub(crate) mod html;
// #[cfg(feature = "html")]
// pub(crate) mod html_item;

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

pub async fn serve(listener:TcpListener) -> anyhow::Result<()>
{
	tracing_subscriber::registry()
		.with(
			tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "rudicom=debug".into()),
		)
		.with(tracing_subscriber::fmt::layer())
		.init();

	// build our application with a route
	let mut app = Router::new()
		.route("/instances",post(handler::store_instance))
		.route("/:table/:id",delete(handler::del_entry))
		.route("/tools/import/json",post(handler::import_json))
		.route("/tools/import/text",post(handler::import_text))
		.route("/studies/json",get(handler::get_studies))
		.route("/:table/:id/json",get(handler::get_entry))
		.route("/:table/:id/json/*query",get(handler::query))
		.route("/:table/:id/parents",get(handler::get_entry_parents))
		.route("/instances/:id/json-ext",get(handler::get_instance_json_ext))
		.route("/instances/:id/file",get(handler::get_instance_file))
		.route("/instances/:id/png",get(handler::get_instance_png))
		.layer(DefaultBodyLimit::max(
			config::get::<usize>("upload_sizelimit_mb").unwrap_or(10)*1024*1024
		))
		;
	#[cfg(feature = "html")]
	{
		app = app
			.route("/studies/html",get(handler::get_studies_html))
			.route("/:table/:id/html",get(handler::get_entry_html))
	}

	// run it
	tracing::info!("listening on {}", listener.local_addr()?);
	tracing::info!("database is {}",db::version().await?);
	tracing::info!("storage path is {}",config::get::<String>("storage_path")?);
	axum::serve(listener,app.into_make_service()).await.map_err(|e|e.into())
}
