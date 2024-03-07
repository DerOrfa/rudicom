use surrealdb::sql;
use thiserror::Error;
use glob::{GlobError,PatternError};

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
	DicomPixelError(#[from] dicom_pixeldata::Error)
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

	#[error("{source} when {context}")]
	Context{
		source:Box<Error>,
		context:String
	},
	#[error("{error}")]
	StringError{
		source:Box<Error>,
		error:String
	},
	#[error("Invalid value type (expected {expected:?}, found {found:?})")]
	UnexpectedResult{
		expected: String,
		found: sql::Value,
	},
	#[error("Entry {id} is not an {expected}")]
	UnexpectedEntry{
		expected: String,
		id: sql::Thing,
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
	IdNotFound{id:sql::Thing},
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
		let context= String::from(context);
		let inner = match self { // if it is context already don't context the context
			Error::Context { source, context:inner_context } => 
			{
				Error::StringError {
					error: inner_context,
					source
				}
			},
			_ => self
		};
		Error::Context {source:Box::new(inner),context:context.into()}
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
