//! Asynchronous results, outputs and responses.
use std::cell::RefCell;
use std::{fmt, thread};
use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::{Value, Error, Version, Id, SyncOutput, SyncResponse, Output, Success, Failure, MethodResult};

/// Convinience type for Synchronous Result
pub type Res = Result<Value, Error>;

/// Asynchronous result receiving end
#[derive(Debug, Clone)]
pub struct AsyncResult {
	result: Arc<Mutex<AsyncResultHandler>>,
}

impl AsyncResult {
	/// Creates new `AsyncResult` (receiver) and `Ready` (transmitter)
	pub fn new() -> (AsyncResult, Ready) {
		let res = Arc::new(Mutex::new(AsyncResultHandler::default()));

		(AsyncResult { result: res.clone() }, Ready { result: res })
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(self, f: F) -> bool where F: FnOnce(Res) + Send + 'static {
		self.result.lock().on_result(f)
	}

	/// Blocks current thread and awaits for result.
	pub fn await(self) -> Res {
		// Check if there is a result already
		{
			let mut result = self.result.lock();
			if let Some(res) = result.try_result() {
				return res;
			}
			// Park the thread
			let current = thread::current();
			// Make sure to keep the lock so result cannot be inserted in between (try_result and notify)
			result.notify(move || {
				current.unpark();
			});
		}

		loop {
			if let Some(res) = self.result.lock().try_result() {
				return res;
			}
			thread::park();
		}
	}
}

impl Into<MethodResult> for AsyncResult {
	fn into(self) -> MethodResult {
		let result = self.result.lock().try_result();
		match result {
			Some(result) => MethodResult::Sync(result),
			None => MethodResult::Async(self)
		}
	}
}

/// Asynchronous result transmitting end
#[derive(Debug, Clone)]
pub struct Ready {
	result: Arc<Mutex<AsyncResultHandler>>,
}

impl Ready {
	/// Submit asynchronous result
	pub fn ready(self, result: Result<Value, Error>) {
		self.result.lock().set_result(result);
	}
}

enum ListenerType {
	Result(Box<FnMut(Res) + Send>),
	Notify(Box<FnMut() + Send>),
}

#[derive(Default)]
struct AsyncResultHandler {
	result: Option<Res>,
	listener: Option<ListenerType>,
}

impl fmt::Debug for AsyncResultHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(formatter, "AsyncResult: {:?}", self.result)
	}
}

impl AsyncResultHandler {
	/// Set result
	pub fn set_result(&mut self, res: Res) {
		// set result
		match self.listener.take() {
			Some(ListenerType::Result(mut listener)) => listener(res),
			Some(ListenerType::Notify(mut notifier)) => {
				self.result = Some(res);
				notifier();
			},
			None => {
				self.result = Some(res);
			}
		}
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(&mut self, f: F) -> bool where F: FnOnce(Res) + Send + 'static {
		if let Some(result) = self.result.take() {
			f(result);
			true
		} else {
			let listener = RefCell::new(Some(f));
			self.listener = Some(ListenerType::Result(Box::new(move |res| {
				listener.borrow_mut().take().unwrap()(res);
			})));
			false
		}
	}

	fn notify<F>(&mut self, f: F) where F: FnOnce() + Send + 'static {
		assert!(self.result.is_none());
		let notifier = RefCell::new(Some(f));
		self.listener = Some(ListenerType::Notify(Box::new(move || {
			notifier.borrow_mut().take().unwrap()();
		})));
	}

	fn try_result(&mut self) -> Option<Res> {
		self.result.take()
	}

}

/// Asynchronous `Output`
#[derive(Debug)]
pub struct AsyncOutput {
	result: AsyncResult,
	id: Id,
	jsonrpc: Version,
}

impl AsyncOutput {
	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(self, f: F) -> bool where F: FnOnce(SyncOutput) + Send + 'static {
		let id = self.id;
		let jsonrpc = self.jsonrpc;
		self.result.on_result(move |result| {
			f(SyncOutput::from(result, id, jsonrpc))
		})
	}

	/// Blocks current thread and awaits a result
	pub fn await(self) -> SyncOutput {
		let result = self.result.await();
		SyncOutput::from(result, self.id, self.jsonrpc)
	}

	/// Create new `AsyncOutput` given `AsyncResult`, `Id` and `Version`
	pub fn from(result: AsyncResult, id: Id, jsonrpc: Version) -> Self {
		AsyncOutput {
			result: result,
			id: id,
			jsonrpc: jsonrpc,
		}
	}
}

/// Response type (potentially asynchronous)
#[derive(Debug)]
pub enum Response {
	/// Single response
	Single(Output),
	/// Batch response
	Batch(Vec<Output>),
}

impl Response {
	/// Blocks current thread and awaits a result.
	pub fn await(self) -> SyncResponse {
		match self {
			Response::Single(Output::Sync(output)) => SyncResponse::Single(output),
			Response::Single(Output::Async(output)) => SyncResponse::Single(output.await()),
			Response::Batch(outputs) => {
				let mut res = Vec::new();
				for output in outputs.into_iter() {
					match output {
						// unwrap sync responses
						Output::Sync(output) => res.push(output),
						// await response
						Output::Async(output) => res.push(output.await()),
					};
				}
				SyncResponse::Batch(res)
			},
		}
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(self, f: F) -> bool where F: FnOnce(SyncResponse) + Send + 'static {
		match self {
			Response::Single(Output::Sync(output)) => {
				f(SyncResponse::Single(output));
				true
			},
			Response::Single(Output::Async(output)) => {
				output.on_result(move |res| f(SyncResponse::Single(res)))
			},
			Response::Batch(outputs) => {
				let mut async = true;
				let mut count = 0;
				// First count async requests
				for output in &outputs {
					if let Output::Async(_) = *output {
						async = true;
						count += 1;
					}
				}

				// Then assign callbacks
				let callback = Arc::new(Mutex::new(Some(f)));
				let responses = Arc::new(Mutex::new(Some(Vec::new())));
				let count = Arc::new(AtomicUsize::new(count));

				for output in outputs {
					match output {
						Output::Async(output) => {
							let count = count.clone();
							let callback = callback.clone();
							let responses = responses.clone();
							output.on_result(move |res| {
								// add response
								{
									let mut response = responses.lock();
									response.as_mut().expect("Callback called only once.").push(res);
								}
								// Check if it's the last output resolved
								let result = count.fetch_sub(1, Ordering::Relaxed);
								if result == 1 {
									let callback = callback.lock().take().expect("Callback called only once.");
									let responses = responses.lock().take().expect("Callback called only once.");
									callback(SyncResponse::Batch(responses));
								}
							});
						},
						Output::Sync(output) => {
							let mut res = responses.lock();
							res.as_mut().expect("Callback called only once").push(output);
						},
					}
				}

				// If there are no async calls just fire callback
				if !async {
					let responses = responses.lock().take().expect("Callback called only once.");
					let callback = callback.lock().take().expect("Callback called only once.");
					callback(SyncResponse::Batch(responses));
					true
				} else {
					// if all async calls were already available
					count.load(Ordering::Relaxed) == 0
				}
			}
		}
	}
}

impl From<Failure> for Response {
	fn from(res: Failure) -> Self {
		Response::Single(Output::Sync(SyncOutput::Failure(res)))
	}
}

impl From<Success> for Response {
	fn from(res: Success) -> Self {
		Response::Single(Output::Sync(SyncOutput::Success(res)))
	}
}


#[cfg(test)]
mod tests {
	use std::sync::{Arc, Mutex};
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::thread;
	use super::super::*;


	#[test]
	fn should_wait_for_all_results_in_batch() {
		// given
		let (res1, ready1) = AsyncResult::new();
		let (res2, ready2) = AsyncResult::new();
		let output1 = AsyncOutput::from(res1.clone(), Id::Null, Version::V2);
		let output2 = AsyncOutput::from(res2.clone(), Id::Null, Version::V2);

		let response = Response::Batch(vec![Output::Async(output1), Output::Async(output2)]);
		let val = Arc::new(Mutex::new(None));
		let v = val.clone();
		assert!(!response.on_result(move |result| {
			*v.lock().unwrap() = Some(result);
		}));
		assert_eq!(val.lock().unwrap().is_none(), true);

		// when
		// resolve first
		ready1.ready(Ok(Value::U64(1)));
		assert_eq!(val.lock().unwrap().is_none(), true);
		// resolve second
		ready2.ready(Ok(Value::U64(2)));
		assert_eq!(val.lock().unwrap().is_none(), false);

		// then
		assert_eq!(val.lock().unwrap().as_ref().unwrap(), &SyncResponse::Batch(vec![
			SyncOutput::Success(Success { result: Value::U64(1), id: Id::Null, jsonrpc: Version::V2 }),
			SyncOutput::Success(Success { result: Value::U64(2), id: Id::Null, jsonrpc: Version::V2 }),
		]));
	}

	#[test]
	fn should_call_on_result_if_available() {
		let (res, ready) = AsyncResult::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		ready.ready(Ok(Value::String("hello".into())));

		let val = Arc::new(AtomicBool::new(false));
		let v = val.clone();

		assert!(output.on_result(move |_| { v.store(true, Ordering::Relaxed) }));
		assert_eq!(val.load(Ordering::Relaxed), true);
	}

	#[test]
	fn should_wait_for_output() {
		let (res, ready) = AsyncResult::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		thread::spawn(move || {
			ready.ready(Ok(Value::String("hello".into())));
		});
		assert_eq!(output.await(), SyncOutput::Success(Success {
			id: Id::Null,
			jsonrpc: Version::V2,
			result: Value::String("hello".into()),
		}));
	}

	#[test]
	fn should_return_output_if_available() {
		let (res, ready) = AsyncResult::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		ready.ready(Ok(Value::String("hello".into())));

		assert_eq!(output.await(), SyncOutput::Success(Success {
			id: Id::Null,
			jsonrpc: Version::V2,
			result: Value::String("hello".into()),
		}));
	}
}
