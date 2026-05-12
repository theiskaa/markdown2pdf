mod spec;

use spec::runner;

#[test]
fn commonmark_spec_suite() {
    let result = runner::run();
    runner::print_report(&result);
    runner::print_failure_details(&result, 25);
    assert!(
        result.regressed.is_empty(),
        "{} examples failed that aren't in known_failures.txt — see report above. \
         Either fix the lexer or add the example numbers to tests/spec/known_failures.txt.",
        result.regressed.len()
    );
    assert!(
        result.unexpected_passes.is_empty(),
        "{} examples in known_failures.txt now pass and should be removed: {:?}",
        result.unexpected_passes.len(),
        result.unexpected_passes
    );
}
