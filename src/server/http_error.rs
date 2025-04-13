use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use thiserror::Error;
use crate::tools;

#[derive(Error,Debug)]
pub enum HttpError{
    #[error("internal error {0}")]
    Internal(tools::Error),
    #[error("Bad request {message}")]
    BadRequest {message:String},
}

impl<T> From<T> for HttpError where tools::Error:From<T>
{
    fn from(error: T) -> Self
    {
        HttpError::Internal(error.into())
    }
}

impl HttpError
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
            HttpError::Internal(e) => Self::internal_status_code(e),
            HttpError::BadRequest { .. } => StatusCode::BAD_REQUEST
        }
    }
    pub fn do_trace(&self)
    {
        match self {
            HttpError::Internal(e) => {
                match e {
                    tools::Error::IdNotFound {id} => tracing::debug!("{id} reported as not found"),
                    _ => tracing::error!("internal error {} reported (root cause '{}')", e, e.root_cause()),
                }
            }
            _ => tracing::error!("http error {} reported", self),
        }
    }
}


pub struct TextError(HttpError);
impl IntoResponse for TextError {
    fn into_response(self) -> Response
    {
        self.0.do_trace();
        let status_code = self.0.status_code();
        let sources:Vec<_>=tools::Source { current: Some( &self.0 ) }.map(<dyn std::error::Error>::to_string).collect();
        (
            status_code,
            sources.join("\n")
        ).into_response()
    }
}

impl<T> From<T> for TextError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        TextError(HttpError::from(error))
    }
}

pub struct JsonError(HttpError);
impl IntoResponse for JsonError {
    fn into_response(self) -> Response
    {
        self.0.do_trace();
        let status_code = self.0.status_code();
        let err= match &self.0 {
            HttpError::Internal(e) => serde_json::Value::from(e),
            HttpError::BadRequest {..} => serde_json::Value::String(self.0.to_string()),
        };
        (status_code,Json(err)).into_response()
    }
}

impl<T> From<T> for JsonError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        JsonError(HttpError::from(error))
    }
}
