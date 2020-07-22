//! HTTP client
//!
//! HTTPS support is enabled with the `tls` feature.

use crate::{RpcChannel, RpcError, RpcResult, RpcMessage};
use failure::format_err;
use futures::{Future, FutureExt, TryFutureExt, StreamExt, TryStreamExt};
use hyper::{http, rt, Client, Request, Uri};
use super::RequestBuilder;

/// Create a HTTP Client
pub fn connect<TClient>(url: &str) -> impl Future<Output = RpcResult<TClient>>
where
	TClient: From<RpcChannel>,
{
	let (sender, receiver) = futures::channel::oneshot::channel();
	let url = url.to_owned();

	std::thread::spawn(move || {
		let connect = rt::lazy(move || do_connect(&url).map(|client| {
			if sender.send(client).is_err() {
				panic!("The caller did not wait for the server.");
			}
			Ok(())
		}).compat());
		rt::run(connect);
	});

	receiver
		.map(|res| res.expect("Server closed prematurely.").map(TClient::from))
}

fn do_connect(url: &str) -> impl Future<Output = RpcResult<RpcChannel>> {
	use futures::future::ready;

	let max_parallel = 8;
	let url: Uri = match url.parse() {
		Ok(url) => url,
		Err(e) => return ready(Err(RpcError::Other(e.into()))),
	};

	#[cfg(feature = "tls")]
	let connector = match hyper_tls::HttpsConnector::new(4) {
		Ok(connector) => connector,
		Err(e) => return ready(Err(RpcError::Other(e.into()))),
	};
	#[cfg(feature = "tls")]
	let client = Client::builder().build::<_, hyper::Body>(connector);

	#[cfg(not(feature = "tls"))]
	let client = Client::new();

	let mut request_builder = RequestBuilder::new();

	let (sender, receiver) = futures::channel::mpsc::unbounded();

	use futures01::{Future, Stream};
	let fut = receiver
		.map(Ok)
		.compat()
		.filter_map(move |msg: RpcMessage| {
			let (request, sender) = match msg {
				RpcMessage::Call(call) => {
					let (_, request) = request_builder.call_request(&call);
					(request, Some(call.sender))
				}
				RpcMessage::Notify(notify) => (request_builder.notification(&notify), None),
				RpcMessage::Subscribe(_) => {
					log::warn!("Unsupported `RpcMessage` type `Subscribe`.");
					return None;
				}
			};

			let request = Request::post(&url)
				.header(
					http::header::CONTENT_TYPE,
					http::header::HeaderValue::from_static("application/json"),
				)
				.header(
					http::header::ACCEPT,
					http::header::HeaderValue::from_static("application/json"),
				)
				.body(request.into())
				.expect("Uri and request headers are valid; qed");

			Some(client.request(request).then(move |response| Ok((response, sender))))
		})
		.buffer_unordered(max_parallel)
		.for_each(|(result, sender)| {
			use futures01::future::{self, Either::{A, B}};
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
				if let Some(sender) = sender {
					let response = result
						.and_then(|response| {
							let response_str = String::from_utf8_lossy(response.as_ref()).into_owned();
							super::parse_response(&response_str)
						})
						.and_then(|r| r.1);
					if let Err(err) = sender.send(response) {
						log::warn!("Error resuming asynchronous request: {:?}", err);
					}
				}
				Ok(())
			})
		});

	rt::spawn(fut.map_err(|e: RpcError| log::error!("RPC Client error: {:?}", e)));
	ready(Ok(sender.into()))
}

#[cfg(test)]
mod tests {
	use assert_matches::assert_matches;
	use crate::*;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_http_server::*;
	use std::net::SocketAddr;
	use super::*;

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
		io.add_sync_method("hello", |params: Params| match params.parse::<(String,)>() {
		Ok((msg,)) => Ok(Value::String(format!("hello {}", msg))),
			_ => Ok(Value::String("world".into())),
		});
		io.add_sync_method("fail", |_: Params| Err(Error::new(ErrorCode::ServerError(-34))));
		io.add_notification("notify", |params: Params| {
			let (value,) = params.parse::<(u64,)>().expect("expected one u64 as param");
			assert_eq!(value, 12);
		});

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
		fn hello(&self, msg: &'static str) -> impl Future<Output = RpcResult<String>> {
			self.0.call_method("hello", "String", (msg,))
		}
		fn fail(&self) -> impl Future<Output = RpcResult<()>> {
			self.0.call_method("fail", "()", ())
		}
		fn notify(&self, value: u64) -> RpcResult<()> {
			self.0.notify("notify", (value,))
		}
	}

	#[test]
	fn should_work() {
		crate::logger::init_log();

		// given
		let server = TestServer::serve(id);

		// when
		let run = async {
			let client: TestClient = connect(&server.uri).await?;
			let result = client.hello("http").await?;

			// then
			assert_eq!("hello http", result);
			Ok(()) as RpcResult<_>
		};

		futures::executor::block_on(run).unwrap();
	}

	#[test]
	fn should_send_notification() {
		crate::logger::init_log();

		// given
		let server = TestServer::serve(id);
		let (tx, rx) = std::sync::mpsc::channel();

		// when
		let run = async move {
			let client: TestClient = connect(&server.uri).await.unwrap();
			client.notify(12).unwrap();
			tx.send(()).unwrap();
		};

		let pool = futures::executor::ThreadPool::builder().pool_size(1).create().unwrap();
		pool.spawn_ok(run);
		rx.recv().unwrap();
	}

	#[test]
	fn handles_invalid_uri() {
		crate::logger::init_log();

		// given
		let invalid_uri = "invalid uri";

		// when
		let res: RpcResult<TestClient> = futures::executor::block_on(connect(invalid_uri));

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

		// when
		let run = async {
			let client: TestClient = connect(&server.uri).await?;
			client.fail().await
		};
		let res = futures::executor::block_on(run);

		// then
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

		let run = async {
			let client: TestClient = connect(&server.uri).await?;
			let res = client.hello("http").await;

			if let Err(RpcError::Other(err)) = res {
				if let Some(err) = err.downcast_ref::<hyper::Error>() {
					assert!(err.is_connect(), format!("Expected Connection Error, got {:?}", err))
				} else {
					panic!("Expected a hyper::Error")
				}
			} else {
				panic!("Expected JsonRpcError. Received {:?}", res)
			}

			Ok(()) as RpcResult<_>
		};

		futures::executor::block_on(run).unwrap();
	}
}
