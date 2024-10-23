use glob::{GlobError, PatternError};
use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;
use crate::db::RecordId;

#[derive(Error,Debug)]
pub enum DicomError
{
	#[error("dicom error {0}")]
	DicomTypeError(#[from] dicom::core::value::ConvertValueError),
	#[error("dicom error {0}")]
	DicomAccessError(#[from] dicom::object::AccessError),
	#[error("dicom io error {0}")]
	DicomReadError(#[from] dicom::object::ReadError),
	#[error("dicom io error {0}")]
	DicomWriteError(#[from] dicom::object::WriteError),
	#[error("error decoding pixel data ({0})")]
	DicomPixelError(#[from] dicom::pixeldata::Error)
}

#[derive(Debug)]
pub struct ErrorContext
{
	error:Box<Error>,
	context:String
}

impl ErrorContext{
	pub fn new<T>(error: Error,context:T) -> ErrorContext where String:From<T>
	{
		ErrorContext{ error:Box::new(error),context:String::from(context)}
	}
	fn inner_context(&self) -> Option<&ErrorContext>
	{
		match self.error.as_ref(){
			Error::Context(c) => Some(c),
			_ => None
		}
	}
	/// get the first inner error that is no context
	fn cause(&self) ->  &Error
	{
		let error = self.error.as_ref();
		match error { 
			Error::Context(c) => c.cause(),
			_ => error
		}
	}
}

impl Display for ErrorContext {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		// cause() will make sure we actually grab a base error, not just another context
		// if the inner error actually is a context it will show up as source (and its fmt() will have the same cause)
		write!(f,"{} when {}",self.cause(),self.context)
	}
}

impl std::error::Error for ErrorContext
{
	/// either get the inner error if it is a context or its source if it isn't
	/// 
	/// inner errors that are
	/// - are not contexts themselves are not considered "sources" and thus should be reported on together with the context
	/// - are context are considered full sources and thus returned 
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> 
	{
		match self.inner_context() // if the inner error is a context too, 
		{
			Some(c) => Some(c), //return that
			None => self.error.source(), //otherwise the inner error's source
		}
	}
}

#[derive(Error,Debug)]
pub enum Error
{
	#[error("task error {0}")]
	JoinError(#[from] tokio::task::JoinError),

	#[error("task error {0}")]
	ConfigError(#[from] config::ConfigError),

	#[error("Database error {0}")]
	SurrealError(#[from] surrealdb::Error),

	#[error("Json error {0}")]
	JsonError(#[from] serde_json::Error),

	#[error("io error {0}")]
	IoError(#[from] std::io::Error),

	#[error("string formatting error {0}")]
	StrFmtError(#[from] strfmt::FmtError),

	#[error("filename {name} is invalid")]
	InvalidFilename{name:std::path::PathBuf},

	#[error("{0}")]
	DicomError(#[from] DicomError),

	#[error(transparent)] // we use our own impl Display above
	Context(#[from]ErrorContext),
	
	#[error("Invalid value type (expected {expected:?}, found {found:?})")]
	UnexpectedResult{
		expected: String,
		found: &'static str,
	},
	#[error("Entry {id} is not an {expected}")]
	UnexpectedEntry {
		expected: String,
		id: RecordId,
	},
	#[error("Failed to parse {to_parse} ({source})")]
	ParseError{
		to_parse: String,
		source: Box<dyn std::error::Error + Send + Sync + 'static>,
	},
	#[error("'{element}' is missing in '{parent}'")]
	ElementMissing{element:String,parent:String},
	#[error("Invalid table {table}")]
	InvalidTable{table:String},
	#[error("No data found")]
	NotFound,
	#[error("{id} not found")]
	IdNotFound{id:String},
	#[error("checksum {checksum} for {file} doesn't fit")]
	ChecksumErr{checksum:String,file:String},
	#[error("Globbing pattern error {0}")]
	GlobPatternError(#[from]PatternError),
	#[error("Globbing error {0}")]
	GlobbingError(#[from]GlobError),
}

impl Error {
	pub(crate) fn context<T>(self, context:T) -> Error where String:From<T>
	{
		ErrorContext::new(self,context).into()
	}
	pub(crate) fn context_from<E,T>(error:E,context:T) -> Error where String:From<T>, Error:From<E>
	{
		Error::from(error).context(context)
	}
	pub(crate) fn sources(&self) -> Source<'_> {
		Source { current: Some( self ) }
	}
	pub(crate) fn root_cause(&self) -> &(dyn std::error::Error + 'static) {
		self.sources().last().expect("Error chains can't be empty")
	}

}

pub struct Source<'a> {
	pub current: Option<&'a (dyn std::error::Error + 'static)>,
}

impl<'a> Iterator for Source<'a> {
	type Item = &'a (dyn std::error::Error + 'static);

	fn next(&mut self) -> Option<Self::Item> {
		let current = self.current;
		self.current = self.current.and_then(std::error::Error::source);
		current
	}
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Context{
	type V;
	fn context<C>(self,context:C) -> Result<Self::V> where String:From<C>;
}

impl<T,E> Context for std::result::Result<T,E> where Error:From<E>
{
	type V=T;
	fn context<C>(self, context: C) -> Result<Self::V> where String: From<C> {
		self.map_err(|e|Error::context_from(e,context))
	}
}
