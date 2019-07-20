//! JSON-RPC unix domain socket client implementation.
use crate::{RpcChannel, RpcError};
use futures::prelude::*;
use std::path::Path;
use tokio::codec::{Framed, LinesCodec};
use tokio_uds::UnixStream;

/// Connect to a JSON-RPC UDS server.
pub fn connect<P, T>(path: P) -> impl Future<Item = T, Error = RpcError>
where
	P: AsRef<Path>,
	T: From<RpcChannel>,
{
	UnixStream::connect(path)
		.map(|unix_stream| {
			let (sink, stream) = Framed::new(unix_stream, LinesCodec::new()).split();
			let sink = sink.sink_map_err(|err| RpcError::Other(err.into()));
			let stream = stream.map_err(|err| RpcError::Other(err.into()));
			let (rpc_client, sender) = super::duplex(sink, stream);
			let rpc_client = rpc_client.map_err(|error| eprintln!("{:?}", error));
			tokio::spawn(rpc_client);
			sender.into()
		})
		.map_err(|error| RpcError::Other(error.into()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::*;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_ipc_server::ServerBuilder;
	use serde_json::map::Map;
	use tokio::runtime::Runtime;

	#[test]
	fn should_call_one() {
		let sock_path = "/tmp/json-ipc-test.ipc";
		let mut io = IoHandler::new();
		io.add_method("greeting", |params| {
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
		let mut rt = Runtime::new().unwrap();
		let builder = ServerBuilder::new(io).event_loop_executor(rt.executor());
		let server = builder.start(sock_path).expect("Couldn't open socket");
		let fut = connect(sock_path).and_then(|client: RawClient| {
			let mut map = Map::new();
			map.insert("name".to_string(), "Jeffry".into());
			client.call_method("greeting", Params::Map(map))
		});
		match rt.block_on(fut) {
			Ok(val) => assert_eq!(&val, "Hello Jeffry!"),
			Err(err) => panic!("UDS RPC call failed: {}", err),
		}
		server.close();
		rt.shutdown_now().wait().unwrap();
	}

	#[test]
	fn should_fail_without_server() {
		let mut rt = Runtime::new().unwrap();
		let fut = connect("/tmp/json-ipc-test.ipc").and_then(|client: RawClient| {
			let mut map = Map::new();
			map.insert("name".to_string(), "Bill".into());
			client.call_method("greeting", Params::Map(map))
		});
		match rt.block_on(fut) {
			Err(RpcError::Other(_)) => (),
			Ok(_) => panic!("Expected the call to fail"),
			_ => panic!("Unexpected error type"),
		}
		rt.shutdown_now().wait().unwrap();
	}

	#[test]
	fn should_handle_server_error() {
		let sock_path = "/tmp/json-ipc-test.ipc";
		let mut io = IoHandler::new();
		io.add_method("greeting", |_params| Err(Error::invalid_params("test error")));
		let mut rt = Runtime::new().unwrap();
		let builder = ServerBuilder::new(io).event_loop_executor(rt.executor());
		let server = builder.start(sock_path).expect("Couldn't open socket");
		let fut = connect(sock_path).and_then(|client: RawClient| {
			let mut map = Map::new();
			map.insert("name".to_string(), "Jeffry".into());
			client.call_method("greeting", Params::Map(map))
		});
		match rt.block_on(fut) {
			Err(RpcError::JsonRpcError(err)) => assert_eq!(err.code, ErrorCode::InvalidParams),
			Ok(_) => panic!("Expected the call to fail"),
			_ => panic!("Unexpected error type"),
		}
		server.close();
		rt.shutdown_now().wait().unwrap();
	}
}
