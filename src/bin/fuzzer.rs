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

static INSTACE_FACTOR:u32=100;
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	// url of the server to connect to
	#[arg(long,default_value_t = String::from("http://localhost:3000"))]
	server: String,
}

async fn send_instance(obj:DefaultDicomObject,url:&str) -> Result<()>{
	let buffer=rudicom::storage::async_store::write(&obj,None)?.into_inner();

	let request = Client::new()
		.post(url)
		.body(Body::from(buffer));

	request.send().await?.error_for_status()?;
	print!(".");
	io::stdout().flush().unwrap();
	Ok(())
}

fn set_tag(obj:&mut DefaultDicomObject,new_val:String,tag:Tag) -> Result<()>
{
	let mut e=obj.take_element(tag).unwrap();
	let mut vals:Vec<_>=e.to_str()?.split('.').map(|s|s.to_string()).collect();
	*vals.last_mut().unwrap()=new_val;
	e.update_value(|v|
		*v.primitive_mut().unwrap()=PrimitiveValue::from(vals.join("."))
	);
	obj.put_element(e);
	Ok(())
}

async fn modify_and_send(mut copy:DefaultDicomObject,instance:u32, series:u32, study:u32, url:String) -> Result<()>
{
	set_tag(&mut copy, (study*10*INSTACE_FACTOR+series*INSTACE_FACTOR+instance).to_string(), tags::SOP_INSTANCE_UID)?;
	set_tag(&mut copy, (study*10+series).to_string(), tags::SERIES_INSTANCE_UID)?;
	set_tag(&mut copy, study.to_string(), tags::STUDY_INSTANCE_UID)?;

	send_instance(copy, url.as_str()).await
}

#[tokio::main]
async fn main() -> Result<()>
{
	let path=PathBuf::from("assets/MR000000.IMA");
	let artifact=read_file(path,None).await?;
	let mut tasks=tokio::task::JoinSet::new();

	let args = Cli::parse();
	let url = args.server.clone() + "/instances";
	for study in 0..10 {
		for series in 0..10 {
			for instance in 0..INSTACE_FACTOR
			{
				tasks.spawn(modify_and_send(artifact.clone(),instance,series,study,url.clone()));
				while tasks.len() > 10 {
					tasks.join_next().await.unwrap()??;
				}
			}
		}
	}
	while let Some(t) = tasks.join_next().await {}
	Ok(())
}
