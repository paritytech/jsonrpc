//! HTTP client

use failure::{format_err, Fail};
use futures::{
	future::{self, Either::{A, B}},
	sync::mpsc,
	Future,
	Stream
};
use hyper::{http, rt, Client, Request};
use jsonrpc_core::{self, Call, Error, Id, MethodCall, Output, Params, Response, Version};

use crate::{RpcChannel, RpcError, RpcMessage};
use super::request_response;
use futures::sink::Sink;

/// Create a HTTP Client
pub fn http<TClient>(url: &str) -> impl Future<Item=TClient, Error=RpcError>
where
	TClient: From<RpcChannel>,
{
	let max_parallel = 8;
	let url = url.to_owned();
	let client = Client::new();

	let (sender, receiver) = mpsc::channel(0);

	let fut = receiver
		.map(move |msg: RpcMessage| {
			let request = jsonrpc_core::Request::Single(Call::MethodCall(MethodCall {
				jsonrpc: Some(Version::V2),
				method: msg.method.clone(),
				params: msg.params.clone(),
				id: Id::Num(1), // todo: [AJ] assign num
			}));
			let request_str = serde_json::to_string(&request).expect("Infallible serialization");

			let request = Request::post(&url)
				.header(http::header::CONTENT_TYPE, http::header::HeaderValue::from_static("application/json"))
				.body(request_str.into())
				.unwrap();

			client
				.request(request)
				.then(move |response| Ok((response, msg)))
		})
		.buffer_unordered(max_parallel)
		.for_each(|(result, msg)| {
			let future = match result {
				Ok(ref res) if !res.status().is_success() => {
					log::trace!("http result status {}", res.status());
					A(future::err(
						RpcError::Other(format_err!("Unexpected response status code: {}", res.status()))
					))
				},
				Ok(res) => B(
					res.into_body()
						.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
						.concat2()
				),
				Err(err) => A(future::err(RpcError::Other(err.into()))),
			};
			future.then(|result| {
				let result = result.and_then(|response| {
					let response_str = String::from_utf8_lossy(response.as_ref()).into_owned();
					serde_json::from_str::<Response>(&response_str)
						.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
						.and_then(|response| {
							let output: Output = match response {
								Response::Single(output) => output,
								Response::Batch(_) => unreachable!(),
							};
							let value: Result<serde_json::Value, Error> = output.into();
							value.map_err(|e| RpcError::JsonRpcError(e))
						})
					});

				if let Err(err) = msg.sender.send(result) {
					log::warn!("Error resuming asynchronous request: {:?}", err);
				}
				Ok(())
			})
		});

	rt::lazy(move|| {
		rt::spawn(fut.map_err(|e| log::error!("RPC Client error: {:?}", e)));
		Ok(TClient::from(sender))
	})
}

#[cfg(test)]
mod tests {
	use jsonrpc_core::{IoHandler, Params, Error, ErrorCode, Value};
	use jsonrpc_http_server::*;
	use hyper::rt;
	use super::*;
	use crate::*;
	use std::time::Duration;

	fn id<T>(t: T) -> T {
		t
	}

	fn serve<F: FnOnce(ServerBuilder) -> ServerBuilder>(alter: F) -> (Server, String) {
		let builder = ServerBuilder::new(io())
			.rest_api(RestApi::Unsecure);

		let server = alter(builder).start_http(&"127.0.0.1:0".parse().unwrap()).unwrap();
		let uri = format!("http://{}", server.address());

		(server, uri)
	}

	fn io() -> IoHandler {
		let mut io = IoHandler::default();
		io.add_method("hello", |params: Params| match params.parse::<(String,)>() {
			Ok((msg,)) => Ok(Value::String(format!("hello {}", msg))),
			_ => Ok(Value::String("world".into())),
		});
		io.add_method("fail", |_: Params| Err(Error::new(ErrorCode::ServerError(-34))));

		io
	}

	#[derive(Clone)]
	struct TestClient(TypedClient);

	impl From<RpcChannel> for TestClient {
		fn from(channel: RpcChannel) -> Self {
			TestClient(channel.into())
		}
	}

	impl TestClient {
		fn hello(&self, msg: &'static str) -> impl Future<Item=String, Error=RpcError> {
			self.0.call_method("hello", "String", (msg,))
		}
		fn fail(&self) -> impl Future<Item=(), Error=RpcError> {
			self.0.call_method("fail", "()", ())
		}
	}

	#[test]
	fn should_work() {
		crate::logger::init_log();

		// given
		let (_server, uri) = serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run =
			http(&uri)
				.and_then(|client: TestClient| {
					client.hello("http")
						.and_then(move |result| {
							drop(client);
							let _ = tx.send(result);
							Ok(())
						})
				})
				.map_err(|e| log::error!("RPC Client error: {:?}", e));

		rt::run(run);

		// then
		let result = rx.recv_timeout(Duration::from_secs(3)).unwrap();
		assert_eq!("hello http", result);
	}

	#[test]
	fn handles_server_error() {
		crate::logger::init_log();

		// given
		let (_server, uri) = serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run =
			http(&uri)
				.and_then(|client: TestClient| {
					client
						.fail()
						.then(move |res| {
							let _ = tx.send(res);
							Ok(())
						})
				})
				.map_err(|e| log::error!("RPC Client error: {:?}", e));
		rt::run(run);

		// then
		let res = rx.recv_timeout(Duration::from_secs(3)).unwrap();

		if let Err(RpcError::JsonRpcError(err)) = res {
			assert_eq!(err, Error { code: ErrorCode::ServerError(-34), message: "Server error".into(), data: None })
		} else {
			panic!("Expected JsonRpcError. Received {:?}", res)
		}
	}

	#[test]
	fn client_still_works_after_server_error() {
		crate::logger::init_log();

		// given
		let (_server, uri) = serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run =
			http(&uri)
				.and_then(|client: TestClient| {
					client
						.fail()
						.and_then(move |_fail_res| {
							client.hello("http")
						})
						.and_then(move |res| {
							let _ = tx.send(res);
							Ok(())
						})
				})
				.map_err(|e| log::error!("RPC Client error: {:?}", e));

		rt::run(run);

		// then
		let res = rx.recv_timeout(Duration::from_secs(3)).unwrap();
		assert_eq!(res, "hello http")
	}
}
