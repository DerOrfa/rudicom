use std::mem;
use std::ops::Deref;
use std::sync::Arc;
use surrealdb::{Connection, Surreal};
use surrealdb::method::Transaction;
use tracing::{warn};
use surrealdb::Result;
use tokio::sync::{oneshot, RwLock};
use crate::db::Session::Ready;

/// A guard holding a session.
///
/// It can either be in a current transaction, or ready to begin one.
/// The transaction will be dropped when the guard is dropped or `cancel()` is called.
/// Call `commit()` to commit the transaction. This will consume the guard

pub struct TransactionGuard<C>(Option<(
	oneshot::Sender<std::result::Result<Surreal<C>,Transaction<C>>>,
	Transaction<C>
)>)
where C:Connection;
pub enum Session<C> where C:Connection
{
	Busy(oneshot::Receiver<std::result::Result<Surreal<C>, Transaction<C>>>),
	Ready(Surreal<C>),
}

impl<C> Session<C> where C:Connection {
	pub fn new(parent:&Surreal<C>) -> Self {Ready(parent.clone())}
	pub async fn begin(&mut self) -> Result<TransactionGuard<C>>
	{
		let (tx, rx) = oneshot::channel();
		let t = match mem::replace(self,Self::Busy(rx)){
			Self::Ready(s) => s.begin().await?, // create a new transaction right away
			Session::Busy(rx) => { // a transaction is busy, wait for it to report back
				match rx.await.unwrap() //sender should never drop without sending
				{
					Err(t) => t.cancel().await?.begin().await?, // Transaction was dropped without finishing, cancel it here
					Ok(s) => s.begin().await?, // normal path transaction finished, and we got our Session back
				}
			}
		};
		Ok(TransactionGuard(Some((tx,t))))
	}
}

pub struct ArcSession<C>(Arc<RwLock<Session<C>>>) where C:Connection;

impl<C> ArcSession<C> where C:Connection{
	pub fn new(parent:&Surreal<C>) -> Self {
		ArcSession(Arc::new(RwLock::new(Session::new(parent))))
	}
	pub async fn begin(&mut self) -> Result<TransactionGuard<C>> {
		self.0.write().await.begin().await
	}
	pub fn clone(&self) -> ArcSession<C> {
		ArcSession(self.0.clone())
	}
}

impl<C> TransactionGuard<C> where C:Connection {
	pub async fn commit(mut self) -> Result<()>{
		// option is only there so we can take out the values without moving them
		// and this function consumes self, so None should never happen
		match self.0.take() {
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
		match self.0.take() {
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
		let repl= match self.0.take() {
			Some((sender, t)) => {
				(sender, t.cancel().await?.begin().await?)
			},
			None => { unreachable!() }
		};
		self.0.replace(repl);
		Ok(())
	}
}

impl<C> Deref for TransactionGuard<C> where C:Connection {
	type Target = Transaction<C>;

	fn deref(&self) -> &Self::Target {
		&self.0.as_ref().unwrap().1
	}
}

impl<C> Drop for TransactionGuard<C> where C:Connection
{
	fn drop(&mut self) {
		if let Some((sender,t)) = self.0.take() {
			warn!("Dropping unfinished transaction, sending it back");
			let _ = sender.send(Err(t));
		}
	}
}
