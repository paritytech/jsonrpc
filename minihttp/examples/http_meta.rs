extern crate jsonrpc_core;
extern crate jsonrpc_minihttp_server;

use std::sync::Arc;
use jsonrpc_core::*;
use jsonrpc_core::futures::Future;
use jsonrpc_minihttp_server::*;

#[derive(Clone, Default)]
struct Meta(usize);
impl Metadata for Meta {}

fn main() {
	let mut io = MetaIoHandler::default();
	io.add_method_with_meta("say_hello", |_params: Params, meta: Meta| {
		futures::finished(Value::String(format!("hello: {}", meta.0))).boxed()
	});

	let server = ServerBuilder::new(io)
		.meta_extractor(Arc::new(|req: &Req| {
			Meta(req.header("Origin").map(|v| v.len()).unwrap_or_default())
		}))
		.cors(DomainsValidation::AllowOnly(vec![cors::AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

