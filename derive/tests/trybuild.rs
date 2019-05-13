#[test]
fn compile_test() {
	let t = trybuild::TestCases::new();
	t.compile_fail("tests/ui/*.rs");
	t.pass("tests/run-pass/*.rs");
}
