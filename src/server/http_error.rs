use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use axum::response::{IntoResponse, Response};
use mime::Mime;
use thiserror::Error;
use crate::server::json::{get_mime, is_json};
use crate::tools;

#[derive(Error,Debug)]
pub enum InnerHttpError {
	#[error("internal error {0}")]
	Internal(tools::Error),
	#[error("Bad request {message}")]
	BadRequest {message:String},
}

impl<T> From<T> for InnerHttpError
where tools::Error:From<T>
{
	fn from(error: T) -> Self
	{
		InnerHttpError::Internal(error.into())
	}
}

impl InnerHttpError
{
	fn internal_status_code(error:&tools::Error) -> StatusCode
	{
		let root = error.root_cause();
		let error_code = root.downcast_ref::<tools::Error>().map(
			|e|match *e {
				tools::Error::NotFound | tools::Error::IdNotFound {..} => StatusCode::NOT_FOUND,
				_ => StatusCode::INTERNAL_SERVER_ERROR
			});
		error_code.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
	}
	pub fn status_code(&self) -> StatusCode
	{
		match &self {
			InnerHttpError::Internal(e) => Self::internal_status_code(e),
			InnerHttpError::BadRequest { .. } => StatusCode::BAD_REQUEST
		}
	}
	pub fn do_trace(&self)
	{
		match self {
			InnerHttpError::Internal(e) => {
				match e {
					tools::Error::IdNotFound {id} => tracing::debug!("{id} reported as not found"),
					_ => tracing::error!("internal error {} reported (root cause '{}')", e, e.root_cause()),
				}
			}
			_ => tracing::error!("http error {} reported", self),
		}
	}
}

pub struct HttpError
{
	inner: InnerHttpError,
	mime: Option<Mime>
}

impl HttpError
{
	pub fn new<T>(error:T, headers:&HeaderMap<HeaderValue>)->Self where InnerHttpError:From<T>
	{
		HttpError {inner: InnerHttpError::from(error),mime:get_mime(headers)}
	}
}

impl IntoResponse for HttpError {
	fn into_response(self) -> Response {
		self.inner.do_trace();
		let status_code = self.inner.status_code();
		if self.mime.is_some_and(|m|is_json(&m)) {
			let err= match &self.inner {
				InnerHttpError::Internal(e) => serde_json::Value::from(e),
				InnerHttpError::BadRequest {..} => serde_json::Value::String(self.inner.to_string()),
			};
			(status_code,Json(err)).into_response()
		} else {
			let sources:Vec<_>=tools::Source { current: Some( &self.inner ) }.map(<dyn std::error::Error>::to_string).collect();
			(
				status_code,
				sources.join("\n")
			).into_response()
		}
	}
}

pub trait IntoHttpError{
	type V;
	fn into_http_error(self,headers:&HeaderMap<HeaderValue>) -> Result<Self::V, HttpError>;
}

impl<T,E> IntoHttpError for Result<T,E> where InnerHttpError:From<E>
{
	type V=T;

	fn into_http_error(self, headers: &HeaderMap<HeaderValue>) -> Result<Self::V, HttpError> {
		self.map_err(|err| HttpError::new(err, headers))
	}
}
