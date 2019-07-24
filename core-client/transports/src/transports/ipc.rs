//! JSON-RPC IPC client implementation using Unix Domain Sockets on UNIX-likes
//! and Named Pipes on Windows.

use crate::transports::duplex::duplex;
use crate::{RpcChannel, RpcError};
use futures::prelude::*;
use jsonrpc_server_utils::codecs::StreamCodec;
use parity_tokio_ipc::IpcConnection;
use std::io;
use std::path::Path;
use tokio::codec::Decoder;

/// Connect to a JSON-RPC IPC server.
pub fn connect<P: AsRef<Path>, Client: From<RpcChannel>>(
	path: P,
	reactor: &tokio::reactor::Handle,
) -> Result<impl Future<Item = Client, Error = RpcError>, io::Error> {
	let connection = IpcConnection::connect(path, reactor)?;

	Ok(futures::lazy(move || {
		let (sink, stream) = StreamCodec::stream_incoming().framed(connection).split();
		let sink = sink.sink_map_err(|e| RpcError::Other(e.into()));
		let stream = stream.map_err(|e| RpcError::Other(e.into()));

		let (client, sender) = duplex(sink, stream);

		tokio::spawn(client.map_err(|e| log::warn!("IPC client error: {:?}", e)));
		Ok(sender.into())
	}))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::*;
	use jsonrpc_core::{Error, ErrorCode, IoHandler, Params, Value};
	use jsonrpc_ipc_server::ServerBuilder;
	use parity_tokio_ipc::dummy_endpoint;
	use serde_json::map::Map;
	use tokio::runtime::Runtime;

	#[test]
	fn should_call_one() {
		let mut rt = Runtime::new().unwrap();
		#[allow(deprecated)]
		let reactor = rt.reactor().clone();
		let sock_path = dummy_endpoint();

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
		let builder = ServerBuilder::new(io).event_loop_executor(rt.executor());
		let server = builder.start(&sock_path).expect("Couldn't open socket");

		let client: RawClient = rt.block_on(connect(sock_path, &reactor).unwrap()).unwrap();
		let mut map = Map::new();
		map.insert("name".to_string(), "Jeffry".into());
		let fut = client.call_method("greeting", Params::Map(map));

		// FIXME: it seems that IPC server on Windows won't be polled with
		// default I/O reactor, work around with sending stop signal which polls
		// the server (https://github.com/paritytech/jsonrpc/pull/459)
		server.close();

		match rt.block_on(fut) {
			Ok(val) => assert_eq!(&val, "Hello Jeffry!"),
			Err(err) => panic!("IPC RPC call failed: {}", err),
		}
		rt.shutdown_now().wait().unwrap();
	}

	#[test]
	fn should_fail_without_server() {
		let rt = Runtime::new().unwrap();
		#[allow(deprecated)]
		let reactor = rt.reactor();

		match connect::<_, RawClient>(dummy_endpoint(), reactor) {
			Err(..) => {}
			Ok(..) => panic!("Should not be able to connect to an IPC socket that's not open"),
		}
		rt.shutdown_now().wait().unwrap();
	}

	#[test]
	fn should_handle_server_error() {
		let mut rt = Runtime::new().unwrap();
		#[allow(deprecated)]
		let reactor = rt.reactor().clone();
		let sock_path = dummy_endpoint();

		let mut io = IoHandler::new();
		io.add_method("greeting", |_params| Err(Error::invalid_params("test error")));
		let builder = ServerBuilder::new(io).event_loop_executor(rt.executor());
		let server = builder.start(&sock_path).expect("Couldn't open socket");

		let client: RawClient = rt.block_on(connect(sock_path, &reactor).unwrap()).unwrap();
		let mut map = Map::new();
		map.insert("name".to_string(), "Jeffry".into());
		let fut = client.call_method("greeting", Params::Map(map));

		// FIXME: it seems that IPC server on Windows won't be polled with
		// default I/O reactor, work around with sending stop signal which polls
		// the server (https://github.com/paritytech/jsonrpc/pull/459)
		server.close();

		match rt.block_on(fut) {
			Err(RpcError::JsonRpcError(err)) => assert_eq!(err.code, ErrorCode::InvalidParams),
			Ok(_) => panic!("Expected the call to fail"),
			_ => panic!("Unexpected error type"),
		}
		rt.shutdown_now().wait().unwrap();
	}
}
