use dicom_dictionary_std::uids::*;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use thiserror::Error;
use tracing::error;

#[allow(deprecated)]
pub static ABSTRACT_SYNTAXES: &[&str] = &[ //https://dicom.nema.org/dicom/2013/output/chtml/part04/sect_B.5.html
	CT_IMAGE_STORAGE,
	ENHANCED_CT_IMAGE_STORAGE,
	STANDALONE_CURVE_STORAGE,
	STANDALONE_OVERLAY_STORAGE,
	SECONDARY_CAPTURE_IMAGE_STORAGE,
	ULTRASOUND_IMAGE_STORAGE_RETIRED,
	NUCLEAR_MEDICINE_IMAGE_STORAGE_RETIRED,
	MR_IMAGE_STORAGE,
	ENHANCED_MR_IMAGE_STORAGE,
	MR_SPECTROSCOPY_STORAGE,
	ENHANCED_MR_COLOR_IMAGE_STORAGE,
	ULTRASOUND_MULTI_FRAME_IMAGE_STORAGE_RETIRED,
	COMPUTED_RADIOGRAPHY_IMAGE_STORAGE,
	DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
	DIGITAL_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
	ENCAPSULATED_PDF_STORAGE,
	ENCAPSULATED_CDA_STORAGE,
	ENCAPSULATED_STL_STORAGE,
	GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE,
	POSITRON_EMISSION_TOMOGRAPHY_IMAGE_STORAGE,
	BREAST_TOMOSYNTHESIS_IMAGE_STORAGE,
	BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
	BREAST_PROJECTION_X_RAY_IMAGE_STORAGE_FOR_PROCESSING,
	ENHANCED_PET_IMAGE_STORAGE,
	RT_IMAGE_STORAGE,
	NUCLEAR_MEDICINE_IMAGE_STORAGE,
	ULTRASOUND_MULTI_FRAME_IMAGE_STORAGE,
	MULTI_FRAME_SINGLE_BIT_SECONDARY_CAPTURE_IMAGE_STORAGE,
	MULTI_FRAME_GRAYSCALE_BYTE_SECONDARY_CAPTURE_IMAGE_STORAGE,
	MULTI_FRAME_GRAYSCALE_WORD_SECONDARY_CAPTURE_IMAGE_STORAGE,
	MULTI_FRAME_TRUE_COLOR_SECONDARY_CAPTURE_IMAGE_STORAGE,
	BASIC_TEXT_SR_STORAGE,
	ENHANCED_SR_STORAGE,
	COMPREHENSIVE_SR_STORAGE,
	VERIFICATION,
];

pub const C_STORE_RQ:u16 = 0x0001;
pub const C_GET_RQ:u16 = 0x0010;
pub const C_FIND_RQ:u16 = 0x0020;
pub const C_MOVE_RQ:u16 = 0x0021;
pub const C_ECHO_RQ:u16 = 0x0030;
pub const C_CANCEL_RQ:u16 = 0x0FFF;
pub const N_EVENT_REPORT_RQ:u16 = 0x0100;
pub const N_GET_RQ:u16 = 0x0110;
pub const N_SET_RQ:u16 = 0x0120;
pub const N_ACTION_RQ:u16 = 0x0130;
pub const N_CREATE_RQ:u16 = 0x0140;
pub const N_DELETE_RQ:u16 = 0x0150;

//https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.4.html#table_C.4-1
//https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_C.4.3.html


pub(crate) type Status<T> = Result<StatusOk<T>, StatusFailure>;
#[derive(Debug, Eq, PartialEq)]
pub enum StatusOk<T>
{
	Success(T), //0x0000
	Pending(T), //0xff00,
	Warning(StatusWarning),
}

#[derive(Debug, Clone, Eq, PartialEq, TryFromPrimitive,IntoPrimitive,Error)]
#[repr(u16)]
pub enum StatusWarning {
	#[error("Attribute List warning")]
	AttributeListWarning = 0x0107,
	#[error("Attribute Value out of range")]
	AttributeValueOutOfRange = 0x0116,
	#[error("Warning A")]
	Warning = 0x0001,
	#[error("Coercion of Data Elements")]
	DataCoercion = 0xB000, //bxxx
	#[error("Elements Discarded")]
	ElementsDiscarded = 0xB006,
	#[error("Data Set does not match SOP Class")]
	DataSetDoesNotMatch = 0xB007,
}

#[derive(Debug, Clone, Eq, PartialEq, TryFromPrimitive,IntoPrimitive,Error)]
#[repr(u16)]
pub enum StatusFailure {
	#[error("Failed")]
	Failure = 0x0100, // 01xx (except 0107 and 0116
	#[error("No such attribute")]
	FailureNoSuchAttribute = 0x0105,
	#[error("Invalid Attribute Value")]
	FailureInvalidAttributeValue = 0x0106,
	#[error("Processing Failure")]
	ProcessingFailure = 0x0110,
	#[error("Duplicate SOP Instance")]
	FailureDuplicateSOPInstance = 0x0111,
	#[error("No such SOP Instance")]
	FailureNoSuchSOPInstance = 0x0112,
	#[error("No such Event Type")]
	FailureNoSuchEvent = 0x0113,
	#[error("No such argument")]
	FailureNoSuchArgument = 0x0114,
	#[error("Invalid argument value")]
	FailureInvalidArgument = 0x0115,
	#[error("Invalid SOP Instance")]
	FailureInvalidSOPInstance = 0x0117,
	#[error("No such SOP Class")]
	FailureNoSuchSOPClass = 0x0118,
	#[error("Class-Instance conflict")]
	FailureClassInstance = 0x0119,
	#[error("Missing Attribute")]
	FailureMissingAttribute = 0x0120,
	#[error("Missing Attribute Value")]
	FailureMissingAttributeValue = 0x0121,
	#[error("SOP Class not supported")]
	FailureInvalidSOPClass = 0x0122,
	#[error("No such Action Type")]
	NoSuchActionType = 0x0123,
	#[error("Not authorized")]
	NotAuthorized = 0x0124,
	#[error("Duplicate Message ID")]
	FailureDuplicateOP = 0x0210,
	#[error("Unrecognized operation")]
	UnrecognizedOperation = 0x0211,
	#[error("Mistyped argument")]
	FailureMistypedArgument = 0x0212,
	#[error("Resource limitation")]
	ResourceLimitation = 0x0213,
	#[error("Out of resources")]
	OutOfResourcesA = 0xA100,
	#[error("Move Destination unknown")]
	FailureMoveDestination = 0xA201,
	#[error("Out of resources")]
	OutOfResources = 0xA700,
	#[error("Out of resources for Calculation")]
	OutOfResourcesCalc = 0xA701,
	#[error("Out of resources for operation")]
	OutOfResourcesOp = 0xA702,
	#[error("Data Set does not match SOP Class")]
	SOPClassDoesNotMatch = 0xA900, // 0xA9xx
	#[error("Cannot understand")]
	FailureCannotUnderstand = 0xC000, // Cxxx
	#[error("Operation was cancelled")]
	Canceled = 0xfe00,
}

pub fn match_status(code:u16) -> Status<()>{
	match code {
		0x0000 => return Ok(StatusOk::Success(())),
		0x0001 ..= 0x00FF => return Ok(StatusOk::Warning(StatusWarning::Warning)),
		0xFF00 ..= 0xFFFF => return Ok(StatusOk::Pending(())),
		0xA700 ..= 0xA7ff => return Err(StatusFailure::OutOfResources),
		0xA900 ..= 0xA9ff => return Err(StatusFailure::FailureInvalidSOPClass),
		0xC000 ..= 0xCfff => return Err(StatusFailure::FailureCannotUnderstand),
		_ => {}
	};
	if let Ok(warn) = StatusWarning::try_from(code){
		Ok(StatusOk::Warning(warn))
	} else if let Ok(fail) = StatusFailure::try_from(code) { 
		Err(fail)
	} else {
		error!("Unknown error code: {:04X}", code);
		Err(StatusFailure::Failure)
	}
}

impl<T> From<StatusOk<T>> for Status<T>{fn from(value: StatusOk<T>) -> Self {Ok(value)}}
impl<T> From<StatusWarning> for Status<T>{fn from(value: StatusWarning) -> Self {Ok(StatusOk::Warning(value))}}
impl<T> From<StatusFailure> for Status<T>{fn from(value: StatusFailure) -> Self {Err(value)}}

pub fn get_status<T>(stat:impl Into<Status<T>>) -> u16 {
	match stat.into() {
		Ok(StatusOk::Success(_)) => 0x0000,
		Ok(StatusOk::Pending(_)) => 0xff00,
		Ok(StatusOk::Warning(w)) => w.into(),
		Err(failure) => failure.into(),
	}
}

