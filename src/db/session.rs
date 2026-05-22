use std::fmt::Debug;
use std::{mem, thread};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use futures::{stream, FutureExt, Stream, StreamExt};
use futures::stream::BoxStream;
use rand::random;
use surrealdb::method::Transaction;
use surrealdb::Result;
use surrealdb::{Connection, Surreal};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::task;
use tracing::{error, trace};

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

/// A basic shared Session.
pub struct LocalSessionStream<C>(BoxStream<'static, Result<TransactionGuard<C>>>) where C:Connection ;
impl<C> Stream for LocalSessionStream<C> where C:Connection {
	type Item = Result<TransactionGuard<C>>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.get_mut().0.poll_next_unpin(cx)
	}
}

pub type SharedSessionStream<C>=Arc<Mutex<LocalSessionStream<C>>>;

impl<C> Session<C> for LocalSessionStream<C> where C:Connection {
	fn create(parent: &Surreal<C>, size:u16) -> Self {
		let mut pool = Box::pin(stream::SelectAll::new());
		let source = parent.clone();
		let stream = stream::poll_fn(move |cx| {
			let pool = &mut pool;
			async {
				while pool.len() < size as usize {
					pool.push(SingleSessionStream::new(&source).fuse())
				}
				pool.next().await
			}.boxed().poll_unpin(cx)
		});
		LocalSessionStream(stream.boxed())
	}

	async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		self.next().await.unwrap()
	}
}
impl<C> Session<C> for SharedSessionStream<C> where C:Connection {
	fn create(parent: &Surreal<C>, size:u16) -> Self {
		Arc::new(LocalSessionStream::<C>::create(parent, size).into())
	}
	async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		self.lock().await.next().await.unwrap()
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
