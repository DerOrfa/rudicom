use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error,Debug)]
#[repr(u16)]
pub(crate) enum HttpError{
    #[error("internal error {source}")]
    Internal{source:crate::tools::Error}=500,
    #[error("Not found")]
    NotFound{source:crate::tools::Error}=404,
    #[error("Bad request {message}")]
    BadRequest {message:String}=400,
}

impl<T> From<T> for HttpError where crate::tools::Error:From<T>
{
    fn from(error: T) -> Self
    {
        let error = crate::tools::Error::from(error);
        match &error {
            crate::tools::Error::NotFound => HttpError::NotFound {source:error},
            _ => HttpError::Internal{source:error}
        }
    }
}

pub(crate) struct TextError(HttpError);
impl IntoResponse for TextError {
    fn into_response(self) -> Response
    {
        todo!();
        // tracing::error!("Internal error {} reported (root cause {})", self.0, self.0.root_cause());
        // (
        //     StatusCode::INTERNAL_SERVER_ERROR,
        //     format!("{:#}", self.0)
        // ).into_response()
    }
}

impl<T> From<T> for TextError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        TextError(HttpError::from(error))
    }
}

pub(crate) struct JsonError(HttpError);
impl IntoResponse for JsonError {
    fn into_response(self) -> Response
    {
        todo!();
        // let chain:Vec<_>=self.0.chain().collect();
        // let text_chain:Vec<_> = chain.iter().map(|e|e.to_string()).collect();
        // tracing::error!("Internal error {} reported (root cause {})",chain.first().unwrap(),chain.last().unwrap());
        // if let Some(root_error) = chain.last().unwrap().downcast_ref::<crate::tools::Error>(){
        //     match root_error {
        //         Error::NotFound => {return (StatusCode::NOT_FOUND,Json(text_chain)).into_response()}
        //     }
        // }
        // (StatusCode::INTERNAL_SERVER_ERROR,Json(text_chain)).into_response()
    }
}

impl<T> From<T> for JsonError where HttpError:From<T>
{
    fn from(error: T) -> Self {
        JsonError(HttpError::from(error))
    }
}
