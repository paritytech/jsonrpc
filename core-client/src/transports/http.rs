//! HTTP client

use hyper::{http, Client, Request};
use futures::{Future, Stream};

use crate::{RpcChannel, RpcError};
use super::request_response;

/// Create a HTTP Client
pub fn http<TClient>(url: &str) -> impl Future<Item=TClient, Error=()>
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

	rpc_client
		.map_err(|e| log::error!("RPC Client error: {:?}", e))
		.map(|()| TClient::from(sender))
}

#[cfg(test)]
mod tests {
	use jsonrpc_core::{IoHandler, Params, Error, ErrorCode, Value};
	use jsonrpc_http_server::*;
	use hyper::rt;
	use super::*;
	use crate::*;

	fn id<T>(t: T) -> T {
		t
	}

	fn serve<F: FnOnce(ServerBuilder) -> ServerBuilder>(alter: F) -> Server {
		let builder = ServerBuilder::new(io())
			.cors(DomainsValidation::AllowOnly(vec![
				AccessControlAllowOrigin::Value("parity.io".into()),
				AccessControlAllowOrigin::Null,
			]))
			.cors_max_age(None)
			.rest_api(RestApi::Secure)
			.health_api(("/health", "hello_async"));

		alter(builder).start_http(&"127.0.0.1:0".parse().unwrap()).unwrap()
	}

	fn io() -> IoHandler {
		let mut io = IoHandler::default();
		io.add_method("hello", |params: Params| match params.parse::<(u64,)>() {
			Ok((num,)) => Ok(Value::String(format!("world: {}", num))),
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
			println!("Client hello");
			self.0.call_method("hello", "String", (msg,))
		}
	}

	#[test]
	fn should_work() {
		env_logger::init();
		use std::thread;

		// given
		let _server = serve(id);

		println!("http test");

//		rt::run(rt::lazy(|| {
//			let uri = "127.0.0.1:0";
//			http(uri)
//				.and_then(|client: TestClient| {
//					client.hello("http")
//						.then(|result| {
//							assert_eq!(result.unwrap(), "hello http");
//							drop(client);
//							Ok(())
//						})
//				})
//		}));

		assert!(false);
	}
}
