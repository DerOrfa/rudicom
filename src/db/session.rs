use std::fmt::Debug;
use std::mem;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use futures::future::BoxFuture;
use futures::FutureExt;
use surrealdb::{Connection, Surreal};
use surrealdb::method::Transaction;
use tracing::{debug, error, warn};
use surrealdb::Result;
use tokio::sync::{oneshot, Mutex};

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
pub enum LocalSession<C> where C:Connection + Debug {
	#[default]
	None,
	Waiting(oneshot::Receiver<std::result::Result<Surreal<C>, Transaction<C>>>),
	Ready(Surreal<C>),
	Busy(BoxFuture<'static, Result<Transaction<C>>>),
	Canceled(BoxFuture<'static, Result<Surreal<C>>>)
}

impl<C> LocalSession<C> where C:Connection + Debug {
	pub fn new(parent:&Surreal<C>) -> Self {Self::Ready(parent.clone())}
}

impl<C> Future for LocalSession<C> where C:Connection + Debug + Unpin {
	type Output = Result<TransactionGuard<C>>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>
	{
		let this = self.get_mut();
		*this = match mem::take(this) { // we have to take it, because the future takes ownership
			// original state, we have a client and can ask for a transaction
			LocalSession::Ready(client) =>
				LocalSession::Busy(client.begin().into_future()),
			other => other // ot stuff it back
		};
		loop {
			// we asked for a transaction, poll for answer, return guard
			if let LocalSession::Busy(fut) = this {
				let transaction = ready!(fut.as_mut().poll(cx))?;
				let (tx, rx) = oneshot::channel();
				debug!("Create transaction {tx:?}{rx:?}");
				let guard = TransactionGuard((false,Some((tx, transaction))));
				*this = LocalSession::Waiting(rx);
				return Poll::Ready(Ok(guard)); // we got our guard, return it
			}
			// all set up, now we're waiting
			if let LocalSession::Waiting(rx) = this {
				debug!("Waiting for transaction from {rx:?}");
				let ret = ready!(rx.poll_unpin(cx));
				*this = match ret {
					Err(e) =>{
						error!("{rx:?}:{e}");
						panic!("RecvError");
					}
					// Transaction was dropped without finishing, cancel it here, and wait for the db to return our session
					Ok(Err(t)) =>{
						debug!("Transaction was canceled, waiting for next");
						LocalSession::Canceled(t.cancel().into_future())
					},
					// normal path transaction finished, and we got our Session back, ask again
					Ok(Ok(s)) => {
						debug!("Transaction was commited, waiting for next");
						LocalSession::Busy(s.begin().into_future())
					},
				};
			};
			// wait for the db to return our session from a canceled transaction
			if let LocalSession::Canceled(f) = this {
				debug!("Cancellation confirmed, waiting for next");
				let session = ready!(f.as_mut().poll(cx))?;
				*this = LocalSession::Busy(session.begin().into_future());
			}
		}
	}
}
impl<C> Session<C> for LocalSession<C> where C:Connection + Debug + Unpin {
	async fn begin(&mut self) -> Result<TransactionGuard<C>>
	{
		self.await
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
pub struct TransactionGuard<C>((bool, Option<(
	oneshot::Sender<std::result::Result<Surreal<C>,Transaction<C>>>,
	Transaction<C>
)>)) where C:Connection;
impl<C> TransactionGuard<C> where C:Connection {
	pub async fn commit(mut self) -> Result<()>{
		// option is only there so we can take out the values without moving them
		// and this function consumes self, so None should never happen
		self.0.0=true;
		match self.0.1.take() {
			Some((sender,t)) =>{
				t.commit().await.map(Ok)
					.map(|s|{let _ = sender.send(s);})
			},
			None => {unreachable!()}
		}
	}
	pub async fn cancel(mut self) -> Result<()>{
		// option is only there so we can take out the values without moving them
		// and this function consumes self, so None should never happen
		self.0.0=true;
		match self.0.1.take() {
			Some((sender,t)) =>{
				t.cancel().await.map(Ok)
					.map(|s|{let _ = sender.send(s);})
			},
			None => {unreachable!()}
		}
	}
	pub async fn reset(&mut self) -> Result<()>{
		// option is only there so we can take out the values without moving them
		// and this function consumes self, so None should never happen
		self.0.0=true;
		let repl= match self.0.1.take() {
			Some((sender, t)) => {
				(sender, t.cancel().await?.begin().await?)
			},
			None => { unreachable!() }
		};
		self.0.1.replace(repl);
		Ok(())
	}
}
impl<C> Deref for TransactionGuard<C> where C:Connection {
	type Target = Transaction<C>;

	fn deref(&self) -> &Self::Target {
		&self.0.1.as_ref().unwrap().1
	}
}
impl<C> Drop for TransactionGuard<C> where C:Connection {
	fn drop(&mut self) {
		match &self.0 {
			(true,_) => {} //intentional drop, all good
			(false, Some(_)) => {
				let (sender,t) = self.0.1.take().unwrap();
				warn!("Dropping unfinished transaction, sending it back");
				let _ = sender.send(Err(t));
			}
			_ => {
				error!("Dropping invalid transaction, this is not good ")
			}
		}
	}
}
