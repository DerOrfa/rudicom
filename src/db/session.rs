use std::fmt::Debug;
use std::{mem, thread};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use futures::{FutureExt, Stream, StreamExt};
use futures::stream::{Fuse, SelectAll};
use rand::random;
use surrealdb::method::Transaction;
use surrealdb::Result;
use surrealdb::{Connection, Surreal};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::{spawn, task};
use tracing::{error, trace, warn};

/// A guard holding a session.
///
/// It can either be `Ready` to start a transaction, or `Busy` waiting for the transaction to either finish or be dropped.
///
/// Call `begin()` to get a new transaction. This might await the current transaction to finish.
/// Call `commit()`/`cancel()` on the transaction to finish it. This will send it back and the session becomes ready again.
/// Dropping the transaction will send it back as well. This is equivalent to calling `cancel()` on it.
pub trait Session<C> where C:Connection
{
	fn create(parent:&Surreal<C>, size:u16) -> Self;
	fn begin(&mut self) -> impl Future<Output = Result<TransactionGuard<C>>> + Send;
}

#[derive(Default, Debug)]
struct SingleSession<C> where C:Connection {
	state:Arc<Mutex<SessionState<C>>>,
	id:u16
}

#[derive(Default,Debug)]
enum SessionState<C> where C:Connection {
	#[default]
	None,
	Ready(Surreal<C>),
	Busy(Transaction<C>)
}
impl<C> SingleSession<C> where C:Connection {
	pub fn new(parent:&Surreal<C>) -> Self {
		let state = SessionState::Ready(parent.clone());
		Self{state:Arc::new(state.into()), id: random() }
	}
	async fn begin_if_valid(&self) -> Result<Option<TransactionGuard<C>>>{
		let thread = thread::current().id();
		trace!("{thread:?} session {} locking state {} others", self.id, Arc::<Mutex<SessionState<C>>>::strong_count(&self.state));
		let mut state = self.state.clone().lock_owned().await;
		let p1: *mut _ = state.deref_mut();
		let addr = p1 as usize;
		trace!("{thread:?} session {} taking state from {addr:x}", self.id);
		let taken =  mem::take(state.deref_mut());
		let begin = match taken {
			SessionState::None => { return Ok(None); }
			// transaction finished / or new
			SessionState::Ready(s) => s.begin(),
			// transaction didn't finish, but we got the lock back, so the guard got dropped
			SessionState::Busy(t) =>
			{
				trace!("{thread:?} session {} awaiting transaction on {addr:x}",self.id);
				t.cancel().await?.begin()
			},
		};
		trace!("{thread:?} session {} awaiting begin on {addr:x}",self.id);
		*state = begin.await.map(SessionState::Busy)
			.map(|e|{trace!("{thread:?} session {} {addr:x} is back to busy",self.id);e})
			.map_err(|e|{
				error!("error {e} when getting a new transaction, this Session is bad now..");
				e})?;

		Ok(Some(TransactionGuard(state,self.id)))
	}
}

struct SingleSessionStream<C> where C:Connection {
	inner:SingleSession<C>,
	active_future: Option<Pin<Box<dyn Future<Output=Option<Result<TransactionGuard<C>>>> + Send>>>,
}
impl<C> SingleSessionStream<C> where C:Connection {
	pub fn new(parent:&Surreal<C>) -> Self {
		Self{
			inner:SingleSession::new(parent),
			active_future:None
		}
	}
}
impl<C> Stream for SingleSessionStream<C> where C:Connection {
	type Item = Result<TransactionGuard<C>>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>{
		let this = self.get_mut();
		trace!("{:?} task {} polling session {}", thread::current().id(), task::id(),this.inner.id);
		let res = ready!(this.active_future.get_or_insert_with(
			||{
				trace!("Handing over session {} to a new future",this.inner.id);
				let inner = SingleSession{state:this.inner.state.clone(),id:this.inner.id};
				async move{
					inner.begin_if_valid().await.transpose()
				}.boxed()
			}
		).poll_unpin(cx));
		this.active_future = None;
		match res {
			Some(r) => {
				trace!("{:?} task {} created new transaction on session {}",
					thread::current().id(), task::id(), this.inner.id);
				Poll::Ready(Some(r))
			},
			None => {
				trace!("session {} died", this.inner.id);
				Poll::Ready(None)
			}
		}
	}
}

impl<C> Drop for SingleSessionStream<C> where C:Connection {
	fn drop(&mut self) {
		if Arc::<Mutex<SessionState<C>>>::strong_count(&self.inner.state) > 1{
			warn!("session {} dropped with an active transaction guard",self.inner.id);
		} else {
			let mut locked = self.inner.state.try_lock().unwrap();
			match mem::take(locked.deref_mut()) {
				SessionState::Busy(t) => { // dropped transaction guard, but never dropped
					// tell it to cancel, hand it over to a separate task and hope it finishes
					spawn(t.cancel().into_future());
				}
				_ => {}
			}
		}
	}
}

/// A Stream that can generate multiple Transactions at a time from an internal pool.
pub struct LocalSession<C> where C:Connection
{
	size:u16,
	source: Surreal<C>,
	pool:SelectAll<Fuse<SingleSessionStream<C>>>,
}

impl<C> Session<C> for LocalSession<C> where C:Connection {
	fn create(parent: &Surreal<C>, size:u16) -> LocalSession<C> {
		LocalSession::<C>{
			size,
			source:parent.clone(),
			pool:SelectAll::new(),
		}
	}

	async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		while self.pool.len() < self.size as usize {
			self.pool.push(SingleSessionStream::new(&self.source).fuse())
		}
		self.pool.next().await.unwrap()
	}
}

/// Same as `LocalSessionStream` but can be shared across threads.
///
/// The internal pool will be shared as well.
pub type SharedSession<C>=Arc<Mutex<LocalSession<C>>>;
impl<C> Session<C> for SharedSession<C> where C:Connection {
	fn create(parent: &Surreal<C>, size:u16) -> Self {
		Arc::new(LocalSession::<C>::create(parent, size).into())
	}
	async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		self.lock().await.begin().await
	}
}


/// The transaction guard returned by `Session::begin()`
///
/// It dereferences to surrealdb::method::Transaction and can be finished by calling either `commit()` ot `cancel()`.
/// Dropping it will cancel the transaction.
pub struct TransactionGuard<C>(OwnedMutexGuard<SessionState<C>>,u16) where C:Connection;
impl<C> TransactionGuard<C> where C:Connection {
	pub async fn commit(mut self) -> Result<()>{
		if let SessionState::Busy(t) = mem::take(self.0.deref_mut()) {
			trace!("{:?} commiting a transaction on session {}", thread::current().id(), self.1);
			t.commit().await.map(|t| *self.0 = SessionState::Ready(t))
		} else { panic!("transaction for session {} is already closed",self.1);}
	}
	pub async fn cancel(mut self) -> Result<()>{
		if let SessionState::Busy(t) = mem::take(self.0.deref_mut()) {
			trace!("{:?} cancelling a transaction on session {}", thread::current().id(),self.1);
			t.cancel().await.map(|t| *self.0 = SessionState::Ready(t))
		} else { panic!("transaction for session {} is already closed",self.1);}
	}
}
impl<C> Deref for TransactionGuard<C> where C:Connection {
	type Target = Transaction<C>;

	fn deref(&self) -> &Self::Target {
		if let SessionState::Busy(t) = self.0.deref() {
			t
		} else { panic!("transaction is already closed");}
	}
}
