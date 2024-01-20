use thiserror::Error;


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
}

#[derive(Error,Debug)]
pub enum Error
{
	#[error("Database error {0}")]
	SurrealError(#[from] surrealdb::Error),

	#[error("Json error {0}")]
	JsonError(#[from] serde_json::Error),

	#[error("io error {0}")]
	IoError(#[from] std::io::Error),

	#[error("{0}")]
	DicomError(#[from] DicomError),

	#[error("{source} when {context}")]
	Context{
		source:Box<Error>,
		context:String
	},
	#[error("Invalid value type (expected {expected:?}, found {found:?})")]
	UnexpectedResult{
		expected: String,
		found: surrealdb::sql::Value,
	},
	#[error("Entry {id} is not an {expected}")]
	UnexpectedEntry{
		expected: String,
		id: surrealdb::sql::Thing,
	},
	#[error("'{element}' is missing in '{parent}'")]
	ElementMissing{element:String,parent:String},
	#[error("Invalid table {table}")]
	InvalidTable{table:String},
	#[error("No data found")]
	NotFound,
	#[error("checksum {checksum} for {file} doesn't fit")]
	ChecksumErr{checksum:String,file:String}
}

impl Error {
	pub(crate) fn context<T>(self, context:T) -> Error where String:From<T>
	{
		Error::Context {source:Box::new(self),context:context.into()}
	}
	pub(crate) fn context_from<E,T>(error:E,context:T) -> Error where String:From<T>, Error:From<E>
	{
		Error::from(error).context(context)
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
