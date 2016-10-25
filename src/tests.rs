// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

use jsonrpc_core::IoHandler;
use std::sync::*;
use super::Server;
use std;
use rand::{thread_rng, Rng};

#[cfg(test)]
pub fn dummy_io_handler() -> Arc<IoHandler> {
	use std::sync::Arc;
	use jsonrpc_core::*;

	struct SayHello;
	impl SyncMethodCommand for SayHello {
		fn execute(&self, params: Params) -> Result<Value, Error> {
			let (request_p1, request_p2) = from_params::<(u64, u64)>(params).unwrap();
			let response_str = format!("hello {}! you sent {}", request_p1, request_p2);
			Ok(Value::String(response_str))
		}
	}

	let io = IoHandler::new();
	io.add_method("say_hello", SayHello);
	Arc::new(io)
}

#[cfg(not(windows))]
pub fn dummy_request(addr: &str, buf: &[u8]) -> Vec<u8> {
	use std::io::{Read, Write};
	use mio::*;
	use mio::unix::*;

	let mut poll = Poll::new().unwrap();
	let mut sock = UnixStream::connect(addr).unwrap();
	poll.register(&sock, Token(0), EventSet::writable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
	poll.poll(Some(500)).unwrap();
	sock.write_all(buf).unwrap();
	poll.reregister(&sock, Token(0), EventSet::readable(), PollOpt::edge() | PollOpt::oneshot()).unwrap();
	poll.poll(Some(500)).unwrap();

	let mut buf = Vec::new();
	sock.read_to_end(&mut buf).unwrap_or_else(|_| { 0 });
	buf
}

#[cfg(windows)]
pub fn dummy_request(addr: &str, buf: &[u8]) -> Vec<u8> {
	use std::io::{Read, Write};
	use miow::pipe::NamedPipe;
	use std::fs::OpenOptions;

	NamedPipe::wait(addr, None).unwrap();
	let mut f = OpenOptions::new().read(true).write(true).open(addr).unwrap();
	f.write_all(buf).unwrap();
	f.flush().unwrap();

	let mut buf = vec![0u8; 65536];
	let sz = f.read(&mut buf).unwrap_or_else(|_| { 0 });
	(&buf[0..sz]).to_vec()
}


pub fn random_ipc_endpoint() -> String {
	let name = thread_rng().gen_ascii_chars().take(30).collect::<String>();
	if cfg!(windows) {
		format!(r"\\.\pipe\{}", name)
	}
	else {
		format!(r"/tmp/{}", name)
	}
}

#[test]
pub fn test_reqrep() {
	let addr = random_ipc_endpoint();
	let io = dummy_io_handler();
	let server = Server::new(&addr, &io).unwrap();
	server.run_async().unwrap();
	std::thread::park_timeout(std::time::Duration::from_millis(50));

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;

	assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
pub fn test_reqrep_two_sequental_connections() {
	super::init_log();

	let addr = random_ipc_endpoint();
	let io = dummy_io_handler();
	let server = Server::new(&addr, &io).unwrap();
	server.run_async().unwrap();
	std::thread::park_timeout(std::time::Duration::from_millis(50));

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
	assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 555! you sent 666","id":1}"#;
	assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

	std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
pub fn test_reqrep_three_sequental_connections() {
	let addr = random_ipc_endpoint();
	let io = dummy_io_handler();
	let server = Server::new(&addr, &io).unwrap();
	server.run_async().unwrap();
	std::thread::park_timeout(std::time::Duration::from_millis(50));

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent 23","id":1}"#;
	assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [555, 666], "id": 1}"#;
	String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap();

	let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [9999, 1111], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"hello 9999! you sent 1111","id":1}"#;
	assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
}

#[test]
#[ignore]
pub fn test_reqrep_100_connections() {
	super::init_log();

	let addr = random_ipc_endpoint();
	let io = dummy_io_handler();
	let server = Server::new(&addr, &io).unwrap();
	server.run_async().unwrap();
	std::thread::park_timeout(std::time::Duration::from_millis(50));

	for i in 0..100 {
		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42,"#.to_owned() + &format!("{}", i) + r#"], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello 42! you sent "#.to_owned() + &format!("{}", i)  + r#"","id":1}"#;
		assert_eq!(String::from_utf8(dummy_request(&addr, request.as_bytes())).unwrap(), response.to_string());
	}
}

#[test]
pub fn big_request() {

	let addr = random_ipc_endpoint();
	let io = dummy_io_handler();

	let server = Server::new(&addr, &io).unwrap();
	server.run_async().unwrap();
	std::thread::park_timeout(std::time::Duration::from_millis(50));

	let request = r#"
		{
			"jsonrpc":"2.0",
			"method":"say_hello",
			"params": [
				42,
				0,
				{
					"from":"0xb60e8dd61c5d32be8058bb8eb970870f07233155",
					"gas":"0x2dc6c0",
					"data":"0x606060405260003411156010576002565b6001805433600160a060020a0319918216811790925560028054909116909117905561291f806100406000396000f3606060405236156100e55760e060020a600035046304029f2381146100ed5780630a1273621461015f57806317c1dd87146102335780631f9ea25d14610271578063266fa0e91461029357806349593f5314610429578063569aa0d8146104fc57806359a4669f14610673578063647a4d5f14610759578063656104f5146108095780636e9febfe1461082b57806370de8c6e1461090d57806371bde852146109ed5780638f30435d14610ab4578063916dbc1714610da35780639f5a7cd414610eef578063c91540f614610fe6578063eae99e1c146110b5578063fedc2a281461115a575b61122d610002565b61122d6004808035906020019082018035906020019191908080601f01602080910402602001604051908101604052809392919081815260200183838082843750949650509335935050604435915050606435600154600090600160a060020a03908116339091161461233357610002565b61122f6004808035906020019082018035906020019191908080601f016020809104026020016040519081016040528093929190818152602001838380828437509496505093359350506044359150506064355b60006000600060005086604051808280519060200190808383829060006004602084601f0104600f02600301f1509050019150509081526020016040518091039020600050905042816005016000508560ff1660028110156100025760040201835060010154604060020a90046001604060020a0316116115df576115d6565b6112416004355b604080516001604060020a038316408152606060020a33600160a060020a031602602082015290519081900360340190205b919050565b61122d600435600254600160a060020a0390811633909116146128e357610002565b61125e6004808035906020019082018035906020019191908080601f01602080910402602001604051908101604052809392919081815260200183838082843750949650509335935050505060006000600060006000600060005087604051808280519060200190808383829060006004602084601f0104600f02600301f1509050019150509081526020016040518091039020600050905080600001600050600087600160a060020a0316815260200190815260200160002060005060000160059054906101000a90046001604060020a03169450845080600001600050600087600160a060020a03168152602001908152602001600020600050600001600d9054906101000a90046001604060020a03169350835080600001600050600087600160a060020a0316815260200190815260200160002060005060000160009054906101000a900460ff169250825080600001600050600087600160a060020a0316815260200190815260200160002060005060000160019054906101000a900463ffffffff16915081505092959194509250565b61122d6004808035906020019082018035906020019191908080601f01602080910402602001604051908101604052809392919081815260200183838082843750949650509335935050604435915050606435608435600060006000600060005088604051808280519060200190808383829060006004602084601f0104600f02600301f15090500191505090815260200160405180910390206000509250346000141515611c0e5760405133600160a060020a0316908290349082818181858883f193505050501515611c1a57610002565b6112996004808035906020019082018035906020019191908080601f01602080910402602001604051908101604052809392919081815260200183838082843750949650509335935050604435915050600060006000600060006000600060006000508a604051808280519060200190808383829060006004602084601f0104600f02600301f15090500191505090815260200160405180910390206000509050806001016000508960ff16600281101561000257600160a060020a038a168452828101600101602052604084205463ffffffff1698506002811015610002576040842054606060020a90046001604060020a031697506002811015610002576040842054640100000000900463ffffffff169650600281101561000257604084206001015495506002811015610002576040842054604060020a900463ffffffff169450600281101561000257505060409091205495999498509296509094509260a060020a90046001604060020a0316919050565b61122d6004808035906020019082018035906020019191908080601f016020809104026020016040519081016040528093929190818152602001838380828437509496505050505050506000600060005082604051808280519060200190808383829060006004602084601f0104600f02600301f15090500191505090815260200160405180910390206000509050348160050160005082600d0160009054906101000a900460ff1660ff16600281101561000257600402830160070180546001608060020a0381169093016001608060020a03199390931692909217909155505b5050565b6112e26004808035906020019082018035906020019191908080601f01602080910003423423094734987103498712093847102938740192387401349857109487501938475"
				}
			]
		}
	"#;

	assert!(String::from_utf8(dummy_request(&addr, request.as_bytes())).is_ok());
}
