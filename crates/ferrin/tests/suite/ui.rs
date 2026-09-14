//! Compile-fail and compile-pass cases of `#[ferrin::tool]` (trybuild).
//!
//! Not run on Windows: trybuild builds a separate project under
//! `target/tests/trybuild/` from a cold cache, which exceeded 6 minutes on
//! the `windows-2025` runner; the diagnostics under test do not depend on
//! the platform and are covered by the Linux and macOS jobs.

#[test]
#[cfg_attr(windows, ignore = "trybuild cold build exceeds the Windows CI budget")]
fn tool_macro_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/pass/*.rs");
    cases.compile_fail("tests/ui/fail/*.rs");
}
