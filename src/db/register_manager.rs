use crate::db::RecordId;
use crate::tools::Error::{DicomError, FieldConflict};
use crate::tools::{Context, extract_from_dicom};
use crate::{db, tools};
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use futures::FutureExt;
use futures::future::BoxFuture;
use std::collections::{BTreeMap, LinkedList};
use std::sync::Arc;
use std::time::Duration;
use surrealdb::types as db_types;
use surrealdb::types::SurrealValue;
use tokio::sync::oneshot::Sender;
use tokio::sync::{Mutex, oneshot};
use tokio::time::sleep;

struct QEntry {
	tx: Sender<()>,
	obj: DefaultDicomObject
}

/// A list of instances of the same series to be commited "in bulk"
///
/// Keeps a series signature to detect insertion conflicts early as well as a timer to
/// automatically trigger commit if not done explicitly by the manager
struct Queue
{
	objects: Arc<Mutex<LinkedList<QEntry>>>,
	series_elements:BTreeMap<String,db_types::Value>,
	timer:BoxFuture<'static,()>
}

async fn commit(objs: Arc<Mutex<LinkedList<QEntry>>>){
	todo!()
}
impl Queue
{
	fn new(obj:&DefaultDicomObject, series_elements:&BTreeMap<String,db_types::Value>) -> Queue {
		// trigger my own commit unless I'm commited before
		let objects:Arc<Mutex<LinkedList<QEntry>>> = Default::default();
		let objects_shared = objects.clone();
		let timer = async move { // force commit after 200ms
			sleep(Duration::from_millis(200)).await;
			commit(objects_shared).await
		}.boxed();
		Queue{objects,timer,series_elements:series_elements.clone()}
	}
}
struct RegisterManager {
	queues:BTreeMap<String, Queue>,
}

impl RegisterManager {
	pub async fn register(&mut self,obj:DefaultDicomObject) -> tools::Result<oneshot::Receiver<()>>
	{
		// create oneshot channel to notify caller about result of registry
		let (tx, rx) = oneshot::channel();

		// determine series signature so we can detect conflicts early
		let study_uid = extract_from_dicom(&obj, tags::STUDY_INSTANCE_UID)?;
		let study_id = RecordId::from_study(study_uid.as_ref());
		let series_elements = db::register::prepare_content(
			&obj,
			vec![("study", study_id.0.into_value())],
			&crate::dcm::SERIES_TAGS
		);

		// determine series to group objects so we can insert them in bulk
		let series_uid = extract_from_dicom(&obj, tags::SERIES_INSTANCE_UID)?.to_string();
		let series_id = RecordId::from_series(series_uid.as_ref());

		// get or make a new "bulk"
		let queue = self.queues.entry(series_uid.clone())
			.or_insert_with(||Queue::new(&obj,&series_elements));

		// detect conflict and reject object if necessary
		if queue.series_elements != series_elements{
			todo!();
			return Err(FieldConflict { fields: "".to_string(), id: series_id })
		}

		// insert
		let mut objects = queue.objects.lock().await;
		objects.push_back(QEntry{tx,obj});

		// if bulk is big enough, trigger commit
		if objects.len() >= crate::config::get().limits.max_files as usize{
			drop(objects); // release mutex so commit below can have it
			let objects=self.queues.remove(&series_uid).unwrap().objects;
			// queue-entry for series/bulk will be dropped here and with it the timer
			// so commit won't be triggered from it running out
			tokio::task::spawn(commit(objects));
		};
		Ok(rx)
	}
	pub async fn flush(self) {
		for (_, queue) in self.queues {
			commit(queue.objects).await
		}
	}
}
