use crate::db::RecordId;
use crate::tools::Error::FieldConflict;
use crate::tools::extract_from_dicom;
use crate::{db, tools};
use dicom::dictionary_std::tags;
use dicom::object::DefaultDicomObject;
use std::collections::{BTreeMap, HashMap, LinkedList};
use std::sync::Arc;
use std::time::Duration;
use itertools::Itertools;
use surrealdb::types as db_types;
use surrealdb::types::SurrealValue;
use tokio::spawn;
use tokio::sync::oneshot::Sender;
use tokio::sync::{Mutex, oneshot};


fn btree_diff(a:&BTreeMap<String,db_types::Value>, mut b:BTreeMap<String,db_types::Value>) -> Vec<String> {
	let mut diff = vec![];
	for (k, v) in a {
		if b.remove(k).as_ref() != Some(v) { // remove from b, so that all that remains in b is
			diff.push(k.to_owned());
		}
	}
	// add all what's left in b
	diff.append(&mut b.into_keys().collect());
	diff
}
#[derive(Debug)]
struct QEntry {
	tx: Sender<()>,
	obj: DefaultDicomObject,
}

/// A list of instances of the same series to be commited "in bulk"
///
/// Keeps a series signature to detect insertion conflicts early
#[derive(Clone,Debug,Default)]
struct Queue
{
	objects: Arc<Mutex<LinkedList<QEntry>>>,
	series_elements:BTreeMap<String,db_types::Value>,
}

#[derive(Debug,Clone)]
struct RegisterManager {
	queues:Arc<Mutex<HashMap<String, Queue>>>,
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
			[("study", study_id.0.into_value())],
			&crate::dcm::SERIES_TAGS
		);

		// determine series to group objects so we can insert them in bulk
		let series_uid = extract_from_dicom(&obj, tags::SERIES_INSTANCE_UID)?.to_string();
		let series_id = RecordId::from_series(series_uid.as_ref());

		// get or make a new queue
		let mut queues = self.queues.lock().await;
		let queue = queues.entry(series_uid.clone()).or_insert_with(|| {
			// it's a new queue, make a timeout commit task for it
			let series_uid_shared = series_uid.clone();
			let self_shared = self.clone();
			spawn(async move {
				tokio::time::sleep(Duration::from_millis(200)).await;
				self_shared.commit(series_uid_shared).await;
			});
			Queue::default()
		});

		// detect conflict and reject object if necessary
		let diff = btree_diff(&queue.series_elements, series_elements);
		if !diff.is_empty() {
			return Err(FieldConflict{
				fields: diff.into_iter().map(|d|format!("{}",d)).join(":"),
				id: series_id
			})
		}

		// insert
		let mut objects = queue.objects.lock().await;
		objects.push_back(QEntry{tx,obj});

		// if bulk is big enough, trigger commit
		let self_shared = self.clone();
		if objects.len() >= crate::config::get().limits.max_files as usize{
			spawn(async move {self_shared.commit(series_uid).await});
		};

		Ok(rx)
	}
	pub async fn commit(&self,series_uid:String){
		let objects = self.queues.lock().await.remove(&series_uid).unwrap().objects;
		todo!()
	}
	pub async fn flush(self) {
		// keep the lock short
		let queues = self.queues.lock().await.drain().collect::<Vec<_>>();
		for (series_uid, _) in queues {
			self.commit(series_uid).await
		}
	}
}
