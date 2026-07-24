//! `xtask` — repo-maintenance automation for rhooks.
//!
//! `cargo xtask gen-core` regenerates `hooks-core`'s translated Rust
//! sources (`error.rs`, `tts.rs`, `ls_flags.rs`, `tx_flags.rs`,
//! `sfcodes.rs`, `consts.rs`, `api.rs`, `host.rs`) from the vendored xahaud
//! `hook/*.h` headers (`docs/DESIGN.md` §4), so the hand-translation step is
//! automated: vendor sync -> parse -> [`ir::HookApiSpec`] ->
//! `crates/hooks-core/hook_api.json` -> per-file codegen -> test -> commit.
//! [`ir`] holds the intermediate representation every generator in
//! [`codegen`] consumes; nothing downstream of it touches header text or
//! [`parse`] types directly.
//!
//! This is a host-side, repo-maintenance CLI tool, not guest-facing wasm
//! code (`docs/DESIGN.md` §4/§8): a panic here means a build/dev-loop
//! failure, not a reachable guest-facing bug, so — like `hooks-build`
//! (`crates/hooks-build/src/lib.rs`) — it relaxes two of the workspace's
//! panic-freedom lints:
//!
//! - `clippy::arithmetic_side_effects`: the header parsers in
//!   [`parse`] do plain byte-offset bookkeeping over small (sub-megabyte),
//!   locally-vendored header files, nowhere near `usize::MAX`.
//! - `clippy::indexing_slicing`: those same parsers slice on offsets they
//!   just computed from `find`/`rfind` on the same string, so an
//!   out-of-bounds index would itself be a generator bug worth panicking
//!   on immediately, not a guest-facing failure mode to design around.
//!
//! `clippy::unwrap_used`, `clippy::expect_used`, and `clippy::panic` remain
//! denied, as inherited from the workspace; this crate uses `anyhow`
//! throughout instead.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
#![allow(clippy::print_stderr)]

mod codegen;
mod gen_core;
mod ir;
mod parse;
mod render;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "rhooks repo-maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Regenerate hooks-core's translated sources from the vendored xahaud
    /// headers.
    GenCore {
        /// Verify the generated sources are up to date without writing
        /// anything (CI mode); exits 1 and lists mismatches if not.
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::GenCore { check: true } => gen_core::run_check(),
        Command::GenCore { check: false } => gen_core::run_update(),
    };
    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
