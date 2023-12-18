mod handler;
mod generators;

use axum::routing::get;

pub(crate) fn router() -> axum::Router
{
	axum::Router::new()
		.route("/studies/html",get(handler::get_studies_html))
		.route("/:table/:id/html",get(handler::get_entry_html))
}
