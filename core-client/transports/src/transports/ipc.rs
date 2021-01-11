//! JSON-RPC IPC client implementation using Unix Domain Sockets on UNIX-likes
//! and Named Pipes on Windows.

use crate::transports::duplex::duplex;
use crate::{RpcChannel, RpcError};
use futures::{SinkExt, StreamExt, TryStreamExt};
use jsonrpc_server_utils::codecs::StreamCodec;
use jsonrpc_server_utils::tokio;
use jsonrpc_server_utils::tokio_util::codec::Decoder as _;
use parity_tokio_ipc::Endpoint;
use std::path::Path;

/// Connect to a JSON-RPC IPC server.
pub async fn connect<P: AsRef<Path>, Client: From<RpcChannel>>(path: P) -> Result<Client, RpcError> {
	let connection = Endpoint::connect(path)
		.await
		.map_err(|e| RpcError::Other(Box::new(e)))?;
	let (sink, stream) = StreamCodec::stream_incoming().framed(connection).split();
	let sink = sink.sink_map_err(|e| RpcError::Other(Box::new(e)));
	let stream = stream.map_err(|e| log::error!("IPC stream error: {}", e));

	let (client, sender) = duplex(
		Box::pin(sink),
		Box::pin(
			stream
				.take_while(|x| futures::future::ready(x.is_ok()))
				.map(|x| x.expect("Stream is closed upon first error.")),
		),
	);

	tokio::spawn(client);

	Ok(sender.into())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::*;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_ipc_server::ServerBuilder;
	use parity_tokio_ipc::dummy_endpoint;
	use serde_json::map::Map;

	#[test]
	fn should_call_one() {
		let sock_path = dummy_endpoint();

		let mut io = IoHandler::new();
		io.add_method("greeting", |params| async {
			let map_obj = match params {
				Params::Map(obj) => obj,
				_ => return Err(Error::invalid_params("missing object")),
			};
			let name = match map_obj.get("name") {
				Some(val) => val.as_str().unwrap(),
				None => return Err(Error::invalid_params("no name")),
			};
			Ok(Value::String(format!("Hello {}!", name)))
		});
		let builder = ServerBuilder::new(io);
		let _server = builder.start(&sock_path).expect("Couldn't open socket");

		let client_fut = async move {
			let client: RawClient = connect(sock_path).await.unwrap();
			let mut map = Map::new();
			map.insert("name".to_string(), "Jeffry".into());
			let fut = client.call_method("greeting", Params::Map(map));

			match fut.await {
				Ok(val) => assert_eq!(&val, "Hello Jeffry!"),
				Err(err) => panic!("IPC RPC call failed: {}", err),
			}
		};
		tokio::runtime::Runtime::new().unwrap().block_on(client_fut);
	}

	#[test]
	fn should_fail_without_server() {
		let test_fut = async move {
			match connect::<_, RawClient>(dummy_endpoint()).await {
				Err(..) => {}
				Ok(..) => panic!("Should not be able to connect to an IPC socket that's not open"),
			}
		};

		tokio::runtime::Runtime::new().unwrap().block_on(test_fut);
	}

	#[test]
	fn should_handle_server_error() {
		let sock_path = dummy_endpoint();

		let mut io = IoHandler::new();
		io.add_method("greeting", |_params| async { Err(Error::invalid_params("test error")) });
		let builder = ServerBuilder::new(io);
		let _server = builder.start(&sock_path).expect("Couldn't open socket");

		let client_fut = async move {
			let client: RawClient = connect(sock_path).await.unwrap();
			let mut map = Map::new();
			map.insert("name".to_string(), "Jeffry".into());
			let fut = client.call_method("greeting", Params::Map(map));

			match fut.await {
				Err(RpcError::JsonRpcError(err)) => assert_eq!(err.code, ErrorCode::InvalidParams),
				Ok(_) => panic!("Expected the call to fail"),
				_ => panic!("Unexpected error type"),
			}
		};

		tokio::runtime::Runtime::new().unwrap().block_on(client_fut);
	}
}
