use std::io;
use std::io::Write;
use std::path::PathBuf;
use reqwest;
use clap::Parser;
use dicom::dictionary_std::tags;
use dicom::core::PrimitiveValue;
use dicom::object::{DefaultDicomObject, Tag};
use reqwest::{Body, Client};
use rudicom::storage::async_store::read_file;
use rudicom::Result;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	// url of the server to connect to
	#[arg(long,default_value_t = String::from("http://localhost:3000"))]
	server: String,
}

fn get_id(obj:&DefaultDicomObject,tag:Tag) -> Result<Vec<String>>
{
	Ok(obj.element(tag)?.to_str()?.split('.').map(|s|s.to_string()).collect())
}

#[tokio::main]
async fn main() -> Result<()>
{
	let path=PathBuf::from("assets/MR000000.IMA");
	let artifact=read_file(path,None).await?;
	let mut instance_id = get_id(&artifact,tags::SOP_INSTANCE_UID)?;
	let series_id = get_id(&artifact,tags::SERIES_INSTANCE_UID)?;
	let study_id = get_id(&artifact, tags::STUDY_INSTANCE_UID)?;

	let args = Cli::parse();
	let client=Client::new();

	for i in 0..1000
	{
		*instance_id.last_mut().unwrap()=i.to_string();
		let mut copy=artifact.clone();
		let mut e=copy.take_element(tags::SOP_INSTANCE_UID).unwrap();
		e.update_value(|v|
				*v.primitive_mut().unwrap()=PrimitiveValue::from(instance_id.join("."))
		);
		copy.put_element(e);
		let buffer=rudicom::storage::async_store::write(&copy,None)?.into_inner();

		let request = client
			.post(args.server.clone()+"/instances")
			.body(Body::from(buffer));

		request.send().await?.error_for_status()?;
		print!(".");
		io::stdout().flush().unwrap();
	}
	todo!()
}
