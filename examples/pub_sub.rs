extern crate jsonrpc_core;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use std::collections::HashMap;

use jsonrpc_core::*;

#[derive(Default)]
struct SayHello {
	id: AtomicUsize,
	subscribers: Mutex<HashMap<usize, Arc<AtomicBool>>>
}

impl SubscriptionCommand for SayHello {
    fn execute(&self, subscription: Subscription) {
		match subscription {
			Subscription::Open { params: _params, subscriber } => {
				// Generate new subscription ID
				let id = self.id.fetch_add(1, Ordering::SeqCst);
				let subscriber = subscriber.assign_id(to_value(id));

				// Add to subscribers
				let finished = Arc::new(AtomicBool::new(false));
				self.subscribers.lock().unwrap().insert(id, finished.clone());

				// Spawn a task
				thread::spawn(move || {
					loop {
						// Stop the thread if user unsubscribes
						if finished.load(Ordering::Relaxed) {
							return;
						}
						// Send a message every second
						subscriber.notify(Ok(to_value("Hello World!")));
						thread::sleep(Duration::from_secs(1));
					}
				});

			},
			Subscription::Close { id, ready } => match id {
				Value::U64(id) => {
					// Remove from subscribers
					match self.subscribers.lock().unwrap().remove(&(id as usize)) {
						Some(finished) => {
							// Close subscription
							finished.store(true, Ordering::Relaxed);
							// Send a response
							ready.ready(Ok(to_value(true)));
						},
						None => ready.ready(Err(Error::invalid_request())),
					}
				},
				_ => ready.ready(Err(Error::invalid_request())),
			},
		}
    }
}

fn main() {
	let io = Arc::new(IoHandler::new());
	io.add_subscription("subscribe_hello", "hello_notification", "unsubscribe_hello", SayHello::default());

	let request = r#"{"jsonrpc": "2.0", "method": "subscribe_hello", "params": [42, 23], "id": 1}"#;
	let is_first = AtomicBool::new(true);
	let first = r#"{"jsonrpc":"2.0","result":0,"id":1}"#;
	let hello = r#"{"jsonrpc":"2.0","method":"hello_notification","params":{"result":"Hello World!","subscription":0}}"#;


	let session = io.session();
	session.handle_request(request, move |res| {
		println!("Got response: {:?}", res);

		if is_first.load(Ordering::Relaxed) {
			is_first.store(false, Ordering::Relaxed);
			assert_eq!(res, Some(first.to_string()));
		} else {
			assert_eq!(res, Some(hello.to_string()));
		}
	});

	thread::sleep(Duration::from_secs(2));
}

