use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::Duration;
use reqwest;
use clap::Parser;
use dicom::dictionary_std::tags;
use dicom::core::PrimitiveValue;
use dicom::object::{DefaultDicomObject, Tag};
use reqwest::{Body, Client, RequestBuilder};
use tokio::time::interval;
use rudicom::storage::async_store::read_file;
use rudicom::Result;

#[derive(Default)]
struct UploadInfo {
	count:std::sync::atomic::AtomicUsize,
	size:std::sync::atomic::AtomicU64,
}
static UPLOAD_INFO:OnceLock<UploadInfo>= OnceLock::new();

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	// url of the server to connect to
	#[arg(long,default_value_t = String::from("http://localhost:3000"))]
	server: String,
	/// how many instances to send per series (10 studies with 10 series each will be sent)
	#[arg(default_value = "100")]
	instances:u32
}

async fn status_update()
{
	let mut interval = interval(Duration::from_millis(100));
	let mut last_cnt= UPLOAD_INFO.get().unwrap().count.load(Ordering::Relaxed);

	loop {
		interval.tick().await; // ticks immediately
		let new_cnt= UPLOAD_INFO.get().unwrap().count.load(Ordering::Relaxed);
		let uploads_per_sec=(new_cnt-last_cnt)*10;
		last_cnt=new_cnt;
		let bytes:usize = uploads_per_sec* UPLOAD_INFO.get().unwrap().size.load(Ordering::Relaxed) as usize;
		print!("\r{} uploads {}MB/s",new_cnt,bytes/(1024*1024));
		io::stdout().flush().unwrap();
	}
}

async fn send_instance(obj:DefaultDicomObject,req:RequestBuilder) -> Result<()>{
	let buffer=rudicom::storage::async_store::write(&obj,None)?.into_inner();

	req.body(Body::from(buffer)).send().await?.error_for_status()?;
	UPLOAD_INFO.get().unwrap().count.fetch_add(1, Ordering::Relaxed);
	Ok(())
}

fn set_tag<T,const M:usize>(obj:&mut DefaultDicomObject,new_vals:[T;M],tag:Tag) -> Result<()> where T:ToString
{
	let mut e=obj.take_element(tag).unwrap();
	let vals:Vec<_>=e.to_str()?.split('.').map(|s|s.to_string()).collect();
	let mut vals = Vec::from(vals.split_at(vals.len()-M).0);
	vals.extend(new_vals.into_iter().map(|v|v.to_string()));
	e.update_value(|v|
		*v.primitive_mut().unwrap()=PrimitiveValue::from(vals.join("."))
	);
	obj.put_element(e);
	Ok(())
}

async fn modify_and_send(mut copy:DefaultDicomObject, instance:u32, series:u32, study:u32, req: RequestBuilder) -> Result<()>
{
	set_tag(&mut copy, [study,series,instance], tags::SOP_INSTANCE_UID)?;
	set_tag(&mut copy, [study,series], tags::SERIES_INSTANCE_UID)?;
	set_tag(&mut copy, [study], tags::STUDY_INSTANCE_UID)?;

	send_instance(copy, req).await
}

#[tokio::main]
async fn main() -> Result<()>
{
	let path=PathBuf::from("assets/MR000000.IMA");
	UPLOAD_INFO.get_or_init(||UploadInfo::default()).size.store(std::fs::metadata(&path)?.len(), Ordering::Relaxed);
	let artifact=read_file(path,None).await?;
	let mut tasks=tokio::task::JoinSet::new();


	let args = Cli::parse();
	let request = Client::new().post(args.server.clone() + "/instances");
	tokio::spawn(status_update());
	for study in 0..10 {
		for series in 0..10 {
			for instance in 0..args.instances
			{
				tasks.spawn(
					modify_and_send(artifact.clone(),instance,series,study,request.try_clone().unwrap())
				);
				while tasks.len() > 10 {
					tasks.join_next().await.unwrap()??;
				}
			}
		}
	}
	while let Some(_) = tasks.join_next().await {}
	Ok(())
}
