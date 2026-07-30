//! Compile-time diagnostic tests for `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!`, driven by `trybuild`.
//!
//! Every fixture under `tests/ui/fail/` is compiled and its **exact** rustc
//! output compared against the committed `.stderr` file beside it; every
//! fixture under `tests/ui/pass/` must compile *and run* cleanly. This is
//! the only kind of test that can cover a declaration macro's rejections at
//! all — a `compile_error!` has no runtime for an ordinary `#[test]` to
//! observe, and `compile_fail` doctests prove only that *something* failed,
//! not that the caller was told anything useful.
//!
//! What the pinned `.stderr` files are really guarding is the **wording and
//! the span** of each diagnostic: several of them (the removed comma-form,
//! the `existing`-as-a-binder case) exist precisely because a vaguer,
//! technically-correct error would leave the caller staring at a macro
//! expansion they did not write. Regenerate them deliberately, with
//! `TRYBUILD=overwrite cargo test -p hooks-lib --test ui`, and read the diff
//! — an unexpected span change is the signal this file exists to catch.
//!
//! Fixture output is toolchain-specific; this repo pins one stable version
//! in `rust-toolchain.toml`, which is what makes byte-exact comparison
//! practical here.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail/*.rs");
    t.pass("tests/ui/pass/*.rs");
}
