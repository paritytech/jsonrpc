//! HTTP client
//!
//! HTTPS support is enabled with the `tls` feature.

use super::RequestBuilder;
use crate::{RpcChannel, RpcError, RpcMessage, RpcResult};
use futures::{future, Future, FutureExt, StreamExt, TryFutureExt};
use hyper::{http, Client, Request, Uri};

/// Create a HTTP Client
pub async fn connect<TClient>(url: &str) -> RpcResult<TClient>
where
	TClient: From<RpcChannel>,
{
	let url: Uri = url.parse().map_err(|e| RpcError::Other(Box::new(e)))?;

	let (client_api, client_worker) = do_connect(url).await;
	tokio::spawn(client_worker);

	Ok(TClient::from(client_api))
}

async fn do_connect(url: Uri) -> (RpcChannel, impl Future<Output = ()>) {
	let max_parallel = 8;

	#[cfg(feature = "tls")]
	let connector = hyper_tls::HttpsConnector::new();
	#[cfg(feature = "tls")]
	let client = Client::builder().build::<_, hyper::Body>(connector);

	#[cfg(not(feature = "tls"))]
	let client = Client::new();
	// Keep track of internal request IDs when building subsequent requests
	let mut request_builder = RequestBuilder::new();

	let (sender, receiver) = futures::channel::mpsc::unbounded();

	let fut = receiver
		.filter_map(move |msg: RpcMessage| {
			future::ready(match msg {
				RpcMessage::Call(call) => {
					let (_, request) = request_builder.call_request(&call);
					Some((request, Some(call.sender)))
				}
				RpcMessage::Notify(notify) => Some((request_builder.notification(&notify), None)),
				RpcMessage::Subscribe(_) => {
					log::warn!("Unsupported `RpcMessage` type `Subscribe`.");
					None
				}
			})
		})
		.map(move |(request, sender)| {
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

			client
				.request(request)
				.then(|response| async move { (response, sender) })
		})
		.buffer_unordered(max_parallel)
		.for_each(|(response, sender)| async {
			let result = match response {
				Ok(ref res) if !res.status().is_success() => {
					log::trace!("http result status {}", res.status());
					Err(RpcError::Client(format!(
						"Unexpected response status code: {}",
						res.status()
					)))
				}
				Err(err) => Err(RpcError::Other(Box::new(err))),
				Ok(res) => {
					hyper::body::to_bytes(res.into_body())
						.map_err(|e| RpcError::ParseError(e.to_string(), Box::new(e)))
						.await
				}
			};

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
		});

	(sender.into(), fut)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::*;
	use assert_matches::assert_matches;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_http_server::*;

	fn id<T>(t: T) -> T {
		t
	}

	struct TestServer {
		uri: String,
		server: Option<Server>,
	}

	impl TestServer {
		fn serve<F: FnOnce(ServerBuilder) -> ServerBuilder>(alter: F) -> Self {
			let builder = ServerBuilder::new(io()).rest_api(RestApi::Unsecure);

			let server = alter(builder).start_http(&"127.0.0.1:0".parse().unwrap()).unwrap();
			let uri = format!("http://{}", server.address());

			TestServer {
				uri,
				server: Some(server),
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

		tokio::runtime::Runtime::new().unwrap().block_on(run).unwrap();
	}

	#[test]
	fn should_send_notification() {
		crate::logger::init_log();

		// given
		let server = TestServer::serve(id);

		// when
		let run = async {
			let client: TestClient = connect(&server.uri).await.unwrap();
			client.notify(12).unwrap();
		};

		tokio::runtime::Runtime::new().unwrap().block_on(run);
		// Ensure that server has not been moved into runtime
		drop(server);
	}

	#[test]
	fn handles_invalid_uri() {
		crate::logger::init_log();

		// given
		let invalid_uri = "invalid uri";

		// when
		let fut = connect(invalid_uri);
		let res: RpcResult<TestClient> = tokio::runtime::Runtime::new().unwrap().block_on(fut);

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
		let res = tokio::runtime::Runtime::new().unwrap().block_on(run);

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

		tokio::runtime::Runtime::new().unwrap().block_on(run).unwrap();
	}
}
