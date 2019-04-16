//! Client transport implementations
use failure::format_err;
use futures::{sync::mpsc, Future, Stream, Sink};
use crate::{RpcError, RpcClient, RpcChannel};

pub mod local;
pub mod http;

/// Create a request/response transport with the supplied future
pub fn request_response<F, R>(request_buffer: usize, f: F) -> (
	impl Future<Item=(), Error=RpcError>,
	RpcChannel,
) where
	F: Fn(String) -> R,
	R: Future<Item=String, Error=RpcError>,
{
	let (requests, requests_rx) = mpsc::channel(request_buffer);
	let (responses, responses_rx) = mpsc::channel(0);

	let run= requests_rx
		.map(f)
		.forward(responses.sink_map_err(|_e| ()))
		.map(|_| ());

	let sink = requests
		.sink_map_err(|e| RpcError::Other(e.into()));

	let stream = responses_rx
		.map_err(|()| unreachable!())
		.and_then(|res| res);

	let (rpc_client, sender) = RpcClient::with_channel(sink, stream);

	let client = run
		.map_err(|()| RpcError::Other(format_err!("Transport error"))) // todo: [AJ] check unifying of error types
		.join(rpc_client)
		.map(|((), ())| ());

	(client, sender)
}
