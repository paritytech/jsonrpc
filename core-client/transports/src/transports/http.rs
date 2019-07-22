//! HTTP client
//!
//! HTTPS support is enabled with the `tls` feature.

use super::RequestBuilder;
use crate::{RpcChannel, RpcError, RpcMessage};
use failure::format_err;
use futures::{
	future::{
		self,
		Either::{A, B},
	},
	sync::mpsc,
	Future, Stream,
};
use hyper::{http, rt, Client, Request, Uri};

/// Create a HTTP Client
pub fn connect<TClient>(url: &str) -> impl Future<Item = TClient, Error = RpcError>
where
	TClient: From<RpcChannel>,
{
	let max_parallel = 8;
	let url: Uri = match url.parse() {
		Ok(url) => url,
		Err(e) => return A(future::err(RpcError::Other(e.into()))),
	};

	#[cfg(feature = "tls")]
	let connector = match hyper_tls::HttpsConnector::new(4) {
		Ok(connector) => connector,
		Err(e) => return A(future::err(RpcError::Other(e.into()))),
	};
	#[cfg(feature = "tls")]
	let client = Client::builder().build::<_, hyper::Body>(connector);

	#[cfg(not(feature = "tls"))]
	let client = Client::new();

	let mut request_builder = RequestBuilder::new();

	let (sender, receiver) = mpsc::channel(max_parallel);

	let fut = receiver
		.filter_map(move |msg: RpcMessage| {
			let msg = match msg {
				RpcMessage::Call(call) => call,
				RpcMessage::Subscribe(_) => {
					log::warn!("Unsupported `RpcMessage` type `Subscribe`.");
					return None;
				}
			};
			let (_, request) = request_builder.call_request(&msg);
			let request = Request::post(&url)
				.header(
					http::header::CONTENT_TYPE,
					http::header::HeaderValue::from_static("application/json"),
				)
				.body(request.into())
				.expect("Uri and request headers are valid; qed");
			Some(client.request(request).then(move |response| Ok((response, msg))))
		})
		.buffer_unordered(max_parallel)
		.for_each(|(result, msg)| {
			let future = match result {
				Ok(ref res) if !res.status().is_success() => {
					log::trace!("http result status {}", res.status());
					A(future::err(RpcError::Other(format_err!(
						"Unexpected response status code: {}",
						res.status()
					))))
				}
				Ok(res) => B(res
					.into_body()
					.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
					.concat2()),
				Err(err) => A(future::err(RpcError::Other(err.into()))),
			};
			future.then(|result| {
				let response = result
					.and_then(|response| {
						let response_str = String::from_utf8_lossy(response.as_ref()).into_owned();
						super::parse_response(&response_str)
					})
					.and_then(|r| r.1);
				if let Err(err) = msg.sender.send(response) {
					log::warn!("Error resuming asynchronous request: {:?}", err);
				}
				Ok(())
			})
		});

	B(rt::lazy(move || {
		rt::spawn(fut.map_err(|e| log::error!("RPC Client error: {:?}", e)));
		Ok(TClient::from(sender.into()))
	}))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::*;
	use assert_matches::assert_matches;
	use hyper::rt;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_http_server::*;
	use std::net::SocketAddr;
	use std::time::Duration;

	fn id<T>(t: T) -> T {
		t
	}

	struct TestServer {
		uri: String,
		socket_addr: SocketAddr,
		server: Option<Server>,
	}

	impl TestServer {
		fn serve<F: FnOnce(ServerBuilder) -> ServerBuilder>(alter: F) -> Self {
			let builder = ServerBuilder::new(io()).rest_api(RestApi::Unsecure);

			let server = alter(builder).start_http(&"127.0.0.1:0".parse().unwrap()).unwrap();
			let socket_addr = server.address().clone();
			let uri = format!("http://{}", socket_addr);

			TestServer {
				uri,
				socket_addr,
				server: Some(server),
			}
		}

		fn start(&mut self) {
			if self.server.is_none() {
				let server = ServerBuilder::new(io())
					.rest_api(RestApi::Unsecure)
					.start_http(&self.socket_addr)
					.unwrap();
				self.server = Some(server);
			} else {
				panic!("Server already running")
			}
		}

		fn stop(&mut self) {
			let server = self.server.take();
			if let Some(server) = server {
				server.close();
			}
		}
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
		fn hello(&self, msg: &'static str) -> impl Future<Item = String, Error = RpcError> {
			self.0.call_method("hello", "String", (msg,))
		}
		fn fail(&self) -> impl Future<Item = (), Error = RpcError> {
			self.0.call_method("fail", "()", ())
		}
	}

	#[test]
	fn should_work() {
		crate::logger::init_log();

		// given
		let server = TestServer::serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run = connect(&server.uri)
			.and_then(|client: TestClient| {
				client.hello("http").and_then(move |result| {
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
	fn handles_invalid_uri() {
		crate::logger::init_log();

		// given
		let invalid_uri = "invalid uri";

		// when
		let run = connect(invalid_uri); // rx.recv_timeout(Duration::from_secs(3)).unwrap();
		let res: Result<TestClient, RpcError> = run.wait();

		// then
		assert_matches!(
			res.map(|_cli| unreachable!()), Err(RpcError::Other(err)) => {
				assert_eq!(format!("{}", err), "invalid uri character");
			}
		);
	}

	#[test]
	fn handles_server_error() {
		crate::logger::init_log();

		// given
		let server = TestServer::serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run = connect(&server.uri)
			.and_then(|client: TestClient| {
				client.fail().then(move |res| {
					let _ = tx.send(res);
					Ok(())
				})
			})
			.map_err(|e| log::error!("RPC Client error: {:?}", e));
		rt::run(run);

		// then
		let res = rx.recv_timeout(Duration::from_secs(3)).unwrap();

		if let Err(RpcError::JsonRpcError(err)) = res {
			assert_eq!(
				err,
				Error {
					code: ErrorCode::ServerError(-34),
					message: "Server error".into(),
					data: None
				}
			)
		} else {
			panic!("Expected JsonRpcError. Received {:?}", res)
		}
	}

	#[test]
	fn handles_connection_refused_error() {
		// given
		let mut server = TestServer::serve(id);
		// stop server so that we get a connection refused
		server.stop();
		let (tx, rx) = std::sync::mpsc::channel();

		let client = connect(&server.uri);

		let call = client
			.and_then(|client: TestClient| {
				client.hello("http").then(move |res| {
					let _ = tx.send(res);
					Ok(())
				})
			})
			.map_err(|e| log::error!("RPC Client error: {:?}", e));

		rt::run(call);

		// then
		let res = rx.recv_timeout(Duration::from_secs(3)).unwrap();

		if let Err(RpcError::Other(err)) = res {
			if let Some(err) = err.downcast_ref::<hyper::Error>() {
				assert!(err.is_connect(), format!("Expected Connection Error, got {:?}", err))
			} else {
				panic!("Expected a hyper::Error")
			}
		} else {
			panic!("Expected JsonRpcError. Received {:?}", res)
		}
	}

	#[test]
	#[ignore] // todo: [AJ] make it pass
	fn client_still_works_after_http_connect_error() {
		// given
		let mut server = TestServer::serve(id);

		// stop server so that we get a connection refused
		server.stop();

		let (tx, rx) = std::sync::mpsc::channel();
		let tx2 = tx.clone();

		let client = connect(&server.uri);

		let call = client
			.and_then(move |client: TestClient| {
				client
					.hello("http")
					.then(move |res| {
						let _ = tx.send(res);
						Ok(())
					})
					.and_then(move |_| {
						server.start(); // todo: make the server start on the main thread
						client.hello("http2").then(move |res| {
							let _ = tx2.send(res);
							Ok(())
						})
					})
			})
			.map_err(|e| log::error!("RPC Client error: {:?}", e));

		// when
		rt::run(call);

		let res = rx.recv_timeout(Duration::from_secs(3)).unwrap();
		assert!(res.is_err());

		// then
		let result = rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
		assert_eq!("hello http", result);
	}
}
