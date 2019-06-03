#![allow(dead_code)]

use env_logger::Builder;
use log::LevelFilter;
use std::env;

lazy_static! {
	static ref LOG_DUMMY: bool = {
		let mut builder = Builder::new();
		builder.filter(None, LevelFilter::Info);

		if let Ok(log) = env::var("RUST_LOG") {
			builder.parse_filters(&log);
		}

		if let Ok(_) = builder.try_init() {
			println!("logger initialized");
		}
		true
	};
}

/// Intialize log with default settings
pub fn init_log() {
	let _ = *LOG_DUMMY;
}
