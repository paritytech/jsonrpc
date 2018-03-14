extern crate jsonrpc_minihttp_server;

use jsonrpc_minihttp_server::{cors, ServerBuilder, DomainsValidation, Req};
use jsonrpc_minihttp_server::jsonrpc_core::*;

#[derive(Clone, Default)]
struct Meta(usize);
impl Metadata for Meta {}

fn main() {
	let mut io = MetaIoHandler::default();
	io.add_method_with_meta("say_hello", |_params: Params, meta: Meta| {
		futures::finished(Value::String(format!("hello: {}", meta.0)))
	});

	let server = ServerBuilder::new(io)
		.meta_extractor(|req: &Req| {
			Meta(req.header("Origin").map(|v| v.len()).unwrap_or_default())
		})
		.cors(DomainsValidation::AllowOnly(vec![cors::AccessControlAllowOrigin::Null]))
		.start_http(&"127.0.0.1:3030".parse().unwrap())
		.expect("Unable to start RPC server");

	server.wait().unwrap();
}

