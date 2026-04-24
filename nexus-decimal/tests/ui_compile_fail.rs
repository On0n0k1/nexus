//! Compile-fail tests for unsound `From<IntType>` combinations.
//!
//! Each test in `tests/ui/` is a small program that should fail to compile
//! because `From<IntType>` is not emitted for that (Backing, D, IntType).
//! The matching `.stderr` files capture the expected error. If rustc
//! diagnostics drift, regenerate via `TRYBUILD=overwrite cargo test
//! --test ui_compile_fail` after auditing the new output.

#[test]
fn ui_unsound_combinations_fail_to_compile() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
