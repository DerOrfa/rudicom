use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error,Debug)]
pub(crate) enum HttpError{
    #[error("internal error {0}")]
    Internal(crate::tools::Error),
    #[error("Bad request {message}")]
    BadRequest {message:String},
}

impl<T> From<T> for HttpError where crate::tools::Error:From<T>
{
    fn from(error: T) -> Self
    {
        HttpError::Internal(error.into())
    }
}

impl HttpError
{
    fn internal_status_code(error:&crate::tools::Error) -> StatusCode
    {
        let error_code = error.root_cause().downcast_ref::<crate::tools::Error>().map(
            |e|match e {
                crate::tools::Error::NotFound | crate::tools::Error::IdNotFound {..} => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR
            });
        error_code.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
    pub(crate) fn status_code(&self) -> StatusCode
    {
        match &self {
            HttpError::Internal(e) => Self::internal_status_code(e),
            HttpError::BadRequest { .. } => StatusCode::BAD_REQUEST
        }
    }
    pub(crate) fn do_trace(&self)
    {
        match self {
            HttpError::Internal(e) => tracing::error!("internal error {} reported (root cause {})", e, e.root_cause()),
            _ => tracing::error!("http error {} reported", self),
        }
    }
    pub(crate) fn sources(&self) -> crate::tools::Source<'_> {
        crate::tools::Source { current: Some( self ) }
    }
}


pub(crate) struct TextError(HttpError);
impl IntoResponse for TextError {
    fn into_response(self) -> Response
    {
        self.0.do_trace();
        let status_code = self.0.status_code();
        let sources:Vec<_>=self.0.sources().map(<dyn std::error::Error>::to_string).collect();
        (
            status_code,
            sources.join("\n")
        ).into_response()
    }
}

impl<T> From<T> for TextError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        HttpError::from(error).into()
    }
}

pub(crate) struct JsonError(HttpError);
impl IntoResponse for JsonError {
    fn into_response(self) -> Response
    {
        self.0.do_trace();
        let status_code = self.0.status_code();
        let sources:Vec<_>=self.0.sources().map(|e|{
            serde_json::Value::String(e.to_string())
        }).collect();
        (status_code,Json(sources)).into_response()
    }
}

impl<T> From<T> for JsonError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        HttpError::from(error).into()
    }
}
