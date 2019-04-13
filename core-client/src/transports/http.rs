//! HTTP client

use hyper::{http, Client, Request};
use hyper::rt;
use futures::{sync::mpsc, Future, Stream, Sink};

use crate::{RpcClient, RpcChannel, RpcError};

struct Transport<TSink, TStream, TFuture> {
	sink: TSink,
	stream: TStream,
	run: TFuture,
}

impl<TSink, TStream, TFuture> Transport<TSink, TStream, TFuture>
where
	TSink: Sink<SinkItem=String, SinkError=RpcError>,
	TStream: Stream<Item=String, Error=RpcError>,
	TFuture: Future<Item=(), Error=()>,
{
	pub fn with_channels<F, R>(request_buffer: usize, f: F) -> Self
	where
		F: Fn(String) -> R,
		R: Future<Item=String, Error=RpcError>,
	{
		let (requests, requests_rx) = mpsc::channel(request_buffer);
		let (responses, responses_rx) = mpsc::channel(0);

		let run = requests_rx
			.map(f)
			.forward(responses
				.sink_map_err(|_e| ())
			)
			.map(|_| ())
			;

		let sink = requests.sink_map_err(|e| RpcError::Other(e.into()));
		let stream = responses_rx
			.map_err(|()| unreachable!())
			.and_then(|res| res);

		Transport { sink, stream, run }
	}

	pub fn client<TClient: From<RpcChannel>>(&self) -> (RpcChannel, TClient) {
		let (sender, receiver) = mpsc::channel(0);
		(sender, RpcClient::new(&self.sink, &self.stream, receiver))
	}
}

//fn http_client(url: &str) -> (
//	impl Future<Item = (), Error = ()>,
//	impl Sink<SinkItem = String, SinkError = RpcError>,
//	impl Stream<Item = String, Error = RpcError>
//) {
//	let url = url.to_owned();
//	let client = Client::new();
//
//	let transport = Transport::with_channels(move |request: String| {
//		let request = Request::post(&url)
//			.header(http::header::CONTENT_TYPE, http::header::HeaderValue::from_static("application/json"))
//			.body(request.into())
//			.unwrap();
//
//		client
//			.request(request)
//			.map_err(|e| RpcError::Other(e.into()))
//			.and_then(|res| {
//				// TODO [ToDr] Handle non-200
//				res.into_body()
//					.map_err(|e| RpcError::ParseError(e.to_string(), e.into()))
//					.concat2()
//					.map(|chunk| String::from_utf8_lossy(chunk.as_ref()).into_owned())
//			})
//	})
//
//
//	(
//		future,
//		requests.sink_map_err(|e| RpcError::Other(e.into())),
//		responses_rx
//			.map_err(|()| unreachable!())
//			.and_then(|res| res)
//	)
//}

/// Create a HTTP Client
pub fn http<TClient>(url: &str) -> impl Future<Item=TClient, Error=()>
where
	TClient: Send + From<RpcChannel> + Future<Item=(), Error=RpcError>,
{
	let url = url.to_owned();
	let client = Client::new();

	let transport = Transport::with_channels(8, move |request: String| {
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

	let (sender, rpc_client) = transport.client::<TClient>();

	(
		rt::lazy(move || {
			rt::spawn(transport.run);
			rt::spawn(rpc_client.map_err(|e| {
				log::error!("RPC Client error: {:?}", e);
			}));
			Ok(sender.into())
		})
	)
}
