//! HTTP client

use hyper::{http, Client, Request};
use hyper::rt;
use futures::{Future, Stream};

use crate::{RpcChannel, RpcError};
use super::request_response;

/// Create a HTTP Client
pub fn http<TClient>(url: &str) -> impl Future<Item=TClient, Error=()>
where
	TClient: Send + From<RpcChannel> + Future<Item=(), Error=RpcError>,
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

	(
		rt::lazy(move || {
			rt::spawn(rpc_client.map_err(|e| {
				log::error!("RPC Client error: {:?}", e);
			}));
			Ok(sender.into())
		})
	)
}
