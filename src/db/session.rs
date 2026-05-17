use std::fmt::Debug;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use surrealdb::method::Transaction;
use surrealdb::Result;
use surrealdb::{Connection, Surreal};
use tokio::sync::{Mutex, OwnedMutexGuard};

/// A guard holding a session.
///
/// It can either be `Ready` to start a transaction, or `Busy` waiting for the transaction to either finish or be dropped.
///
/// Call `begin()` to get a new transaction. This might await the current transaction to finish.
/// Call `commit()`/`cancel()` on the transaction to finish it. This will send it back and the session becomes ready again.
/// Dropping the transaction will send it back as well. This is equivalent to calling `cancel()` on it.
pub trait Session<C> where C:Connection
{
	fn begin(&mut self) -> impl Future<Output = Result<TransactionGuard<C>>> + Send;
}

/// A basic local Session.
///
/// This cannot me cloned, but borrowed.

#[derive(Default)]
pub struct LocalSession<C> where C:Connection + Debug {
	state:Arc<Mutex<SessionState<C>>>
}

#[derive(Default)]
enum SessionState<C> where C:Connection {
	#[default]
	None,
	Ready(Surreal<C>),
	Busy(Transaction<C>)
}
impl<C> LocalSession<C> where C:Connection + Debug {
	pub fn new(parent:&Surreal<C>) -> Self {
		let state = SessionState::Ready(parent.clone());
		Self{state:Arc::new(state.into())}
	}
}

impl<C> Session<C> for LocalSession<C> where C:Connection + Debug + Unpin {
	async fn begin(&mut self) -> Result<TransactionGuard<C>>
	{
		let mut lock = self.state.clone().lock_owned().await;
		let transaction = match mem::take(lock.deref_mut()) {
			SessionState::None => {panic!("Invalid state")}
			// transaction finished / or new
			SessionState::Ready(s) => s.begin(),
			// transaction didn't finish, but we got the lock back
			SessionState::Busy(t) => t.cancel().await?.begin(),
		}.await?;
		*lock = SessionState::Busy(transaction);
		Ok(TransactionGuard(lock))
	}
}

/// A basic shared Session.
#[derive(Clone)]
pub struct ArcSession<C>(Arc<Mutex<LocalSession<C>>>) where C:Connection + Debug;
impl<C> ArcSession<C> where C:Connection + Debug {
	pub fn new(parent:&Surreal<C>) -> Self {
		ArcSession(Arc::new(Mutex::new(LocalSession::new(parent))))
	}
}
impl<C> Session<C> for ArcSession<C> where C:Connection + Debug + Unpin {
	async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		self.0.lock().await.begin().await
	}
}


/// The transaction guard returned by `Session::begin()`
///
/// It dereferences to surrealdb::method::Transaction and can be finished by calling either `commit()` ot `cancel()`.
/// Dropping it will cancel the transaction.
pub struct TransactionGuard<C>(OwnedMutexGuard<SessionState<C>>) where C:Connection;
impl<C> TransactionGuard<C> where C:Connection {
	pub async fn commit(mut self) -> Result<()>{
		if let SessionState::Busy(t) = mem::take(self.0.deref_mut()) {
			*self.0 = SessionState::Ready(t.commit().await?);
			Ok(())
		}else { panic!("TransactionGuard was already commited");}
	}
	pub async fn cancel(mut self) -> Result<()>{
		if let SessionState::Busy(t) = mem::take(self.0.deref_mut()) {
			*self.0 = SessionState::Ready(t.cancel().await?);
			Ok(())
		}else { panic!("TransactionGuard was already commited");}
	}
	pub async fn reset(&mut self) -> Result<()>{
		if let SessionState::Busy(t) = mem::take(self.0.deref_mut()) {
			*self.0 = SessionState::Busy(t.cancel().await?.begin().await?);
			Ok(())
		}else { panic!("TransactionGuard was already commited");}
	}
}
impl<C> Deref for TransactionGuard<C> where C:Connection {
	type Target = Transaction<C>;

	fn deref(&self) -> &Self::Target {
		if let SessionState::Busy(t) = self.0.deref() {
			t
		}else { panic!("TransactionGuard was already commited");}
	}
}
