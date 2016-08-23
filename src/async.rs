use std::{mem, fmt, thread};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use super::{Value, Error, Version, Id, SyncOutput, SyncResponse, Output, Success, Failure};

type Res = Result<Value, Error>;

pub struct AsyncResultHandler {
	result: Mutex<Option<Res>>,
	listeners: Mutex<Vec<Box<Fn() + Send>>>
}

impl fmt::Debug for AsyncResultHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(
			formatter,
			"AsyncResult: {:?}, Listeners: {}",
			self.result,
			self.listeners.lock().unwrap().len()
		)
	}
}

impl AsyncResultHandler {
	pub fn new() -> Arc<Self> {
		Arc::new(AsyncResultHandler {
			result: Mutex::new(None),
			listeners: Mutex::new(Vec::new()),
		})
	}

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
			on_result();
		}
	}

	pub fn on_result<F>(&self, f: F) where F: Fn() + Send + 'static {
		if self.result.lock().unwrap().is_some() {
			f();
		} else {
			self.listeners.lock().unwrap().push(Box::new(f));
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

pub type AsyncResult = Arc<AsyncResultHandler>;

#[derive(Debug)]
pub struct AsyncOutput {
	result: AsyncResult,
	id: Id,
	jsonrpc: Version,
}

impl AsyncOutput {
	pub fn await(self) -> SyncOutput {
		let result = self.result.await();
		SyncOutput::from(result, self.id, self.jsonrpc)
	}

	pub fn on_result<F>(&self, f: F) where F: Fn() + Send + 'static {
		self.result.on_result(f)
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
	pub fn on_result<F>(&self, f: F) where F: Fn() + Send + Sync + 'static {
		match *self {
			Response::Single(Output::Sync(_)) => {
				f();
			},
			Response::Single(Output::Async(ref output)) => {
				output.on_result(f);
			},
			Response::Batch(ref outputs) => {
				let mut async = true;
				let callback = Arc::new(f);
				let count = Arc::new(AtomicUsize::new(0));
				for output in outputs {
					if let Output::Async(ref output) = *output {
						async = true;
						count.fetch_add(1, Ordering::Relaxed);

						let count = count.clone();
						let callback = callback.clone();
						output.on_result(move || {
							let res = count.fetch_sub(1, Ordering::Relaxed);
							// Last output resolved
							if res == 1 {
								callback();
								return;
							}
						});
					}
				}
				if !async {
					callback()
				}
			}
		}
	}

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
	use std::sync::Arc;
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::thread;
	use super::super::*;
	use super::AsyncResultHandler;


	#[test]
	fn should_wait_for_all_results_in_batch() {
		// given
		let res1 = AsyncResultHandler::new();
		let res2 = AsyncResultHandler::new();
		let output1 = AsyncOutput::from(res1.clone(), Id::Null, Version::V2);
		let output2 = AsyncOutput::from(res2.clone(), Id::Null, Version::V2);

		let response = Response::Batch(vec![Output::Async(output1), Output::Async(output2)]);
		let val = Arc::new(AtomicBool::new(false));
		let v = val.clone();
		response.on_result(move || { v.store(true, Ordering::Relaxed) });
		assert_eq!(val.load(Ordering::Relaxed), false);

		// when
		// resolve first
		res1.set_result(Ok(Value::U64(1)));
		assert_eq!(val.load(Ordering::Relaxed), false);
		// resolve second
		res2.set_result(Ok(Value::U64(2)));
		assert_eq!(val.load(Ordering::Relaxed), true);

		// then
		assert_eq!(response.await(), SyncResponse::Batch(vec![
			SyncOutput::Success(Success { result: Value::U64(1), id: Id::Null, jsonrpc: Version::V2 }),
			SyncOutput::Success(Success { result: Value::U64(2), id: Id::Null, jsonrpc: Version::V2 }),
		]));
	}

	#[test]
	fn should_call_on_result_if_available() {
		let res = AsyncResultHandler::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		res.set_result(Ok(Value::String("hello".into())));

		let val = Arc::new(AtomicBool::new(false));
		let v = val.clone();
		res.on_result(move || { v.store(true, Ordering::Relaxed) });

		assert_eq!(val.load(Ordering::Relaxed), true);
	}

	#[test]
	fn should_wait_for_output() {
		let res = AsyncResultHandler::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		thread::spawn(move || {
			res.set_result(Ok(Value::String("hello".into())));
		});
		assert_eq!(output.await(), SyncOutput::Success(Success {
			id: Id::Null,
			jsonrpc: Version::V2,
			result: Value::String("hello".into()),
		}));
	}

	#[test]
	fn should_return_output_if_available() {
		let res = AsyncResultHandler::new();
		let output = AsyncOutput::from(res.clone(), Id::Null, Version::V2);
		res.set_result(Ok(Value::String("hello".into())));

		assert_eq!(output.await(), SyncOutput::Success(Success {
			id: Id::Null,
			jsonrpc: Version::V2,
			result: Value::String("hello".into()),
		}));
	}
}
