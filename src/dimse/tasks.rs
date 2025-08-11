use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll, Waker};
use std::task::Poll::Ready;
use futures::Stream;
use tokio::task::JoinHandle;
use crate::dimse::message::Message;
use crate::dimse::payload::SendPayload;
use crate::tools::Result;

pub(crate) struct TaskStack<T:Future<Output=Result<SendPayload>>>{
	stack:std::collections::LinkedList<(Message,JoinHandle<T::Output>)>,
	waker:Option<Waker>,
	closed:bool,
}

impl<T: Future<Output=Result<SendPayload>>> Default for TaskStack<T>
{
	fn default() -> Self {TaskStack{
		stack: Default::default(),
		waker: None,
		closed: false,
	}}
}

impl<T: Future<Output=Result<SendPayload>> + Send + 'static> TaskStack<T> {
	pub fn push(&mut self, m:Message, future: T) {
		self.stack.push_back((m,tokio::spawn(future)));
		self.update();
	}
	pub fn close(&mut self) {self.closed = true;self.update()}
	fn update(&mut self) {
		if let Some(waker) = &self.waker {waker.wake_by_ref();}
	}
}

/// polls the task on top of the stack and removes it, if (and only if) it's done
/// sends pending when empty unless closed
impl<T> Stream for TaskStack<T> where T:Future<Output=Result<SendPayload>>
{
	type Item = Result<SendPayload>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		if self.closed {return Ready(None);}
		self.waker=Some(cx.waker().clone()); // keep last waker, so we can signal to it when adding tasks
		match self.stack.back_mut() {
			None => Poll::Pending, // send pending if stack is empty
			Some((_,  f)) => { // poll "uppermost" task
				let p = pin!(f).poll(cx).map(|r|Some(r.unwrap()));
				if p.is_ready(){self.stack.pop_back();} // if task is done, remove it from the stack
				p
			},
		}
	}
}
