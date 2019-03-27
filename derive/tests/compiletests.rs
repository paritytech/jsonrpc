use compiletest_rs as compiletest;

use std::path::PathBuf;

fn run_mode(mode: &'static str) {
	let mut config = compiletest::Config::default();

	config.mode = mode.parse().expect("Invalid mode");
	config.src_base = PathBuf::from(format!("tests/{}", mode));
	config.target_rustcflags = Some(String::from(
		"\
		 -L ../target/debug/ \
		 -L ../target/debug/deps/ \
		 ",
	));
	config.clean_rmeta(); // If your tests import the parent crate, this helps with E0464

	compiletest::run_tests(&config);
}

#[test]
fn compile_test() {
	run_mode("ui");
	run_mode("run-pass");
}
