use crate::db::{register, RecordId, Session, SharedSession, DB};
use crate::tools::Error::FieldConflict;
use crate::tools::extract_from_dicom;
use crate::{db, storage, tools};
use dicom::dictionary_std::tags;
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap, LinkedList};
use std::sync::Arc;
use std::time::Duration;
use surrealdb::engine::any::Any;
use surrealdb::{types as db_types, Connection};
use surrealdb::types::SurrealValue;
use tokio::spawn;
use tokio::sync::oneshot::Sender;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinSet;
use tracing::error;

fn btree_diff(a: &BTreeMap<String, db_types::Value>, mut b:BTreeMap<String,db_types::Value>) -> Vec<String> {
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

/// An entry for the insertion queue.
///
/// Consists of an image and the Sender part of a oneshot channel to notify about the eventual result of the insert
#[derive(Debug)]
pub struct QEntry {
	pub tx: Sender<tools::Result<bool>>,
	pub image: crate::storage::Image,
}

/// A list of QEntries to be commited "in bulk".
///
/// They are expected to belong to the same series.
/// A series signature is kept to detect insertion conflicts early.
#[derive(Debug,Default)]
struct Queue
{
	objects: LinkedList<QEntry>,
	series_elements:BTreeMap<String,db_types::Value>,
}

#[derive(Clone)]
struct RegisterManager {
	queues:Arc<Mutex<HashMap<String, Queue>>>,
	session:SharedSession<Any>,
}

/// Manages inserts for a specific transaction.
///
/// Dropping it without calling [flush] will drop all uncommited data and end the transaction.
/// Call [commit] to commit data for a specific series.
impl RegisterManager {
	pub fn new() -> Self {
		Self{ queues: Arc::new(Default::default()), session: SharedSession::create(&DB, 5) }
	}
	/// Collect images to be inserted in bulk.
	///
	/// They will automatically be grouped by series.
	/// Commits will automatically be done once the limit for open files is reached or 200ms since
	/// first addition for the given group.
	///
	/// returns a `oneshot::Receiver` that can be awaited to get notice of the insertion result.
	/// Dropping this Receiver will trigger a warning message, but insertion will still be done if possible.
	pub async fn register(&mut self,image:storage::Image) -> tools::Result<oneshot::Receiver<tools::Result<bool>>>
	{
		// create oneshot channel to notify caller about result of registry
		let (tx, rx) = oneshot::channel();

		// determine series signature so we can detect conflicts early
		let study_uid = extract_from_dicom(image.as_ref(), tags::STUDY_INSTANCE_UID)?;
		let study_id = RecordId::from_study(study_uid.as_ref());
		let series_elements = db::register::prepare_content(
			image.as_ref(),
			[("study", study_id.0.into_value())],
			&crate::dcm::SERIES_TAGS
		);

		// determine series to group objects so we can insert them in bulk
		let series_uid = extract_from_dicom(image.as_ref(), tags::SERIES_INSTANCE_UID)?.to_string();

		// get or make a new queue
		let mut queues = self.queues.lock().await;
		let queue = queues.entry(series_uid.clone()).or_insert_with(|| {
			// it's a new queue, make a timeout commit task for it
			let series_uid_shared = series_uid.clone();
			let mut self_shared = self.clone();
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
				id: RecordId::from_series(series_uid.as_ref())
			})
		}

		// insert
		queue.objects.push_back(QEntry{tx, image });

		// if bulk is big enough, trigger commit
		let mut self_shared = self.clone();
		if queue.objects.len() >= crate::config::get().limits.max_files as usize{
			spawn(async move {self_shared.commit(series_uid).await});
		};

		Ok(rx)
	}

	/// runs bulk_insert on a list of Queue entries and bisects the list recursively if an error occurs
	/// returns list of entries that where successful inserted
	/// receivers are noticed about fails
	async fn inner_commit<S,C>(mut entries:Vec<QEntry>, session: &mut S) -> Vec<QEntry> where S:Session<C>, C:Connection
	{
		if let Err(e) =  register::bulk_insert(&mut entries, session).await { // something is bad,
			// if its just one entry
			if entries.len()<=1{ // tell its receiver
				entries.pop().map(|entry|entry.tx.send(Err(e)));
				// entries is empty now / the image is dropped
			} else { // bisect entries and try again
				let b = entries.drain(entries.len()/2 ..).collect(); // take off half
				entries = Box::pin(Self::inner_commit(entries,session)).await; // run first half
				entries.append(&mut Box::pin(Self::inner_commit(b,session)).await); //add other half again
			}
		}
		entries
	}

	/// removes an image group from the queue and commits it
	/// failing entries are dropped as well as their images
	pub async fn commit(&mut self, series_uid:String) {
		// if triggered by a timeout the queue might actually be gone already
		if let Some(mut entries) = self.queues.lock().await.remove(&series_uid).map(|q|q.objects)
		{
			// First save all images as uncommitted //////////////////////////
			let mut saver = JoinSet::new();
			let mut saved_entries = vec![];
			let save_fn = async move |entry:QEntry| {
				match entry.image.into_saved().await {
					Ok(image) => Some(QEntry { tx: entry.tx, image }),
					Err(e) => {
						entry.tx.send(Err(e)).unwrap();
						None
					},
				}
			};
			// fill up joinset
			while saver.len() < crate::config::get().limits.max_files as usize {
				if let Some(entry) = entries.pop_front() {
					saver.spawn(save_fn(entry));
				} else { break; }
			}
			// join one / add one until all are done
			while let Some(result) = saver.join_next().await {
				match result {
					Ok(Some(entry)) => saved_entries.push(entry),
					Err(e) => error!("Task to write image failed: {e}"),
					_ => {},
				}
				if let Some(new)= entries.pop_front(){
					saver.spawn(save_fn(new));
				}
			}

			// next commit to db /////////////////////////////////////////////
			let commited_entries = Self::inner_commit(saved_entries,&mut self.session).await;

			// last, commit the files now and let receivers know /////////////
			commited_entries.into_iter().for_each(|mut entry|{
				entry.image.commit();
				if let Err(_) = entry.tx.send(Ok(true)){
					error!("failed to let receiver know about successful insert")
				}
			});
		}
	}
	pub async fn flush(mut self) {
		// keep the lock short
		let queues = self.queues.lock().await.drain().collect::<Vec<_>>();
		for (series_uid, _) in queues {
			self.commit(series_uid).await
		}
	}
}
