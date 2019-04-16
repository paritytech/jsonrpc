//! Client transport implementations
use futures::{sync::mpsc, Future, Stream, Sink};
use crate::{RpcClient, RpcChannel, RpcError};

pub mod http;

pub fn request_response<F, R>(request_buffer: usize, f: F) -> (
	impl Sink<SinkItem=String, SinkError=RpcError>,
	impl Stream<Item=String, Error=RpcError>,
	impl Future<Item=(), Error=()>,
) where
	F: Fn(String) -> R,
	R: Future<Item=String, Error=RpcError>,
{
	let (requests, requests_rx) = mpsc::channel(request_buffer);
	let (responses, responses_rx) = mpsc::channel(0);

	let run = requests_rx
		.map(f)
		.forward(responses.sink_map_err(|_e| ()))
		.map(|_| ());

	let sink = requests
		.sink_map_err(|e| RpcError::Other(e.into()));

	let stream = responses_rx
		.map_err(|()| unreachable!())
		.and_then(|res| res);

	(sink, stream, run)
}
