use std::{mem, fmt, thread};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use super::{Value, Error, Version, Id, SyncOutput, SyncResponse, Output, Success, Failure};

type Res = Result<Value, Error>;

#[derive(Debug, Clone)]
pub struct AsyncResult {
	result: Arc<AsyncResultHandler>,
}

impl AsyncResult {
	pub fn new() -> (AsyncResult, Ready) {
		let res = Arc::new(AsyncResultHandler {
			result: Mutex::new(None),
			listeners: Mutex::new(Vec::new()),
		});

		(AsyncResult { result: res.clone() }, Ready { result: res })
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(self, f: F) -> bool where F: Fn(Res) + Send + 'static {
		let result = self.result.clone();
		self.result.on_result(move || {
			f(result.await())
		})
	}

	pub fn await(self) -> Res {
		self.result.await()
	}
}



#[derive(Debug, Clone)]
pub struct Ready {
	result: Arc<AsyncResultHandler>,
}

impl Ready {
	pub fn ready(self, result: Result<Value, Error>) {
		self.result.set_result(result);
	}
}


struct AsyncResultHandler {
	result: Mutex<Option<Res>>,
	listeners: Mutex<Vec<Box<Fn() + Send>>>
}

impl fmt::Debug for AsyncResultHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(
			formatter,
			"AsyncResult: {:?}",
			*self.result.lock().unwrap(),
		)
	}
}

impl AsyncResultHandler {
	pub fn set_result(&self, res: Res) {
		{
			let mut result = self.result.lock().unwrap();
			*result = Some(res);
		}

		let listeners = {
			let mut listeners = self.listeners.lock().unwrap();
			mem::replace(&mut *listeners, Vec::new())
		};

		for on_result in listeners.into_iter() {
			on_result()
		}
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(&self, f: F) -> bool where F: Fn() + Send + 'static {
		let has_result = self.result.lock().unwrap().is_some();
		if has_result {
			f();
			true
		} else {
			self.listeners.lock().unwrap().push(Box::new(f));
			false
		}
	}

	pub fn await(&self) -> Res {
		// Check if there is a result already
		{
			if let Some(ref res) = *self.result.lock().unwrap() {
				return res.clone();
			}
		}
		// Park the thread
		let current = thread::current();
		self.on_result(move || {
			current.unpark();
		});

		loop {
			if let Some(ref res) = *self.result.lock().unwrap() {
				return res.clone();
			}
			thread::park();
		}
	}


}

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
	pub fn on_result<F>(self, f: F) -> bool where F: Fn(SyncOutput) + Send + 'static {
		let id = self.id;
		let jsonrpc = self.jsonrpc;
		self.result.on_result(move |result| {
			f(SyncOutput::from(result, id.clone(), jsonrpc.clone()))
		})
	}

	pub fn await(self) -> SyncOutput {
		let result = self.result.await();
		SyncOutput::from(result, self.id, self.jsonrpc)
	}

	pub fn from(result: AsyncResult, id: Id, jsonrpc: Version) -> Self {
		AsyncOutput {
			result: result,
			id: id,
			jsonrpc: jsonrpc,
		}
	}
}

#[derive(Debug)]
pub enum Response {
	Single(Output),
	Batch(Vec<Output>),
}

impl Response {
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
	pub fn on_result<F>(self, f: F) -> bool where F: Fn(SyncResponse) + Send + 'static {
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
									let mut response = responses.lock().unwrap();
									response.as_mut().expect("Callback called only once.").push(res);
								}
								// Check if it's the last output resolved
								let result = count.fetch_sub(1, Ordering::Relaxed);
								if result == 1 {
									let callback = callback.lock().unwrap().take().expect("Callback called only once.");
									let responses = responses.lock().unwrap().take().expect("Callback called only once.");
									callback(SyncResponse::Batch(responses));
								}
							});
						},
						Output::Sync(output) => {
							let mut res = responses.lock().unwrap();
							res.as_mut().expect("Callback called only once").push(output);
						},
					}
				}

				// If there are no async calls just fire callback
				if !async {
					let responses = responses.lock().unwrap().take().expect("Callback called only once.");
					let callback = callback.lock().unwrap().take().expect("Callback called only once.");
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
