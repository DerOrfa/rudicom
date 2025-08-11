mod dispatcher;
mod message;
mod io;
pub(crate) mod definitions;
mod payload;
mod tasks;

use dicom_ul::FullAeAddr;
use tokio::net::TcpListener;
use tokio::sync::watch::Sender;
use tracing::{debug, error};
use crate::tools::Result;

// Specs
// https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.html

pub async fn serve(listener:TcpListener, name: impl AsRef<str>, watcher: Sender<()>) -> Result<()>
{
	tracing::info!("SCP \"{}\" listening on: tcp://{}", name.as_ref(), listener.local_addr()?);

	loop {
		let (stream,addr) = tokio::select! {
			Ok(conn) = listener.accept() => conn,
			_ = watcher.closed() => {
				tracing::trace!("signal received, not accepting new connections");
				break;
			}
		};
		let title = FullAeAddr::new(name.as_ref(),addr);
		let dispatcher = dispatcher::Dispatcher::new(&name,stream,watcher.clone()).await.unwrap();
		tokio::spawn(async move {
			if let Err(e) = dispatcher.run().await {
				error!("{title} aborted with error: \"{}\"", e);
			}
		}
		);
	}
	Ok(())
}
