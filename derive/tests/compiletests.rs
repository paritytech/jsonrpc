#[test]
fn compile_test() {
	let t = trybuild::TestCases::new();
	t.compile_fail("ui/*.rs");
	t.pass("run-pass/*.rs");
}
