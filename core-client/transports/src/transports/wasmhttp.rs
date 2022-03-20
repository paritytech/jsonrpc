//! HTTP client for wasm via gloo_net
use super::RequestBuilder;
use gloo_net::http;
use futures::{future, Future, FutureExt, StreamExt, TryFutureExt};
use crate::{RpcChannel, RpcError, RpcMessage, RpcResult};


/// Create a HTTP Client via gloo_net for wasm
pub async fn connect<TClient>(url: &str) -> RpcResult<(TClient, impl Future<Output = ()> + '_)>
where TClient: From<RpcChannel>
{
    let max_parallel = 8;
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
            log::info!("got request {:?}", request);
			let request = http::Request::post(&url)
				.header(
					"Content-Type",
					"application/json",
				)
				.header(
					"Accept",
					"application/json",
				)
				.body(request);

			request.send()
				.then(|response| async move { (response, sender) })
		})
		.buffer_unordered(max_parallel)
		.for_each(|(response, sender)| async {
			let result = match response {
				Ok(ref res) if !res.ok() => {
					log::trace!("http result status {}", res.status());
					Err(RpcError::Client(format!(
						"Unexpected response status code: {}",
						res.status()
					)))
				}
				Err(err) => Err(RpcError::Other(Box::new(err))),
				Ok(res) => {
					res.binary()
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

    Ok((TClient::from(sender.into()), fut))
}
