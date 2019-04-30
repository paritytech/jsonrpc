//! HTTP client

use hyper::{http, rt, Client, Request};
use futures::{Future, Stream};

use crate::{RpcChannel, RpcError};
use super::request_response;

/// Create a HTTP Client
pub fn http<TClient>(url: &str) -> impl Future<Item=TClient, Error=RpcError>
where
	TClient: From<RpcChannel>,
{
	let url = url.to_owned();
	let client = Client::new();

	let (rpc_client, sender) = request_response(8, move |request: String| {
		let request = Request::post(&url)
			.header(http::header::CONTENT_TYPE, http::header::HeaderValue::from_static("application/json"))
			.body(request.into())
			.unwrap();

		client
			.request(request)
			.map_err(|e| RpcError::Other(e.into()))
			.and_then(|res| {
				// TODO [ToDr] Handle non-200
				res.into_body()
					.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
					.concat2()
					.map(|chunk| String::from_utf8_lossy(chunk.as_ref()).into_owned())
			})
	});

	rt::lazy(move|| {
		rt::spawn(rpc_client.map_err(|e| log::error!("RPC Client error: {:?}", e)));
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
			panic!("Server should error")
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
