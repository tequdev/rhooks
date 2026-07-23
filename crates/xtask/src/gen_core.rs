//! Orchestrates `cargo xtask gen-core`: reads the vendored xahaud headers
//! (`crates/hooks-core/vendor/xahaud-hook/`), runs each per-file generator
//! in [`crate::codegen`], formats the output with `rustfmt` under the repo's
//! `rustfmt.toml`, and either writes the result into
//! `crates/hooks-core/src/` or (`--check`) compares it against what's
//! already there without touching the working tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::codegen;

/// The set of files this generator owns. `lib.rs` is deliberately excluded
/// (`docs/DESIGN.md` §4): it's hand-wired module/re-export plumbing, not a
/// header translation, and the spec calls it out as NOT generated.
const GENERATED_FILES: &[&str] = &[
    "error.rs",
    "tts.rs",
    "ls_flags.rs",
    "tx_flags.rs",
    "sfcodes.rs",
    "consts.rs",
    "api.rs",
];

/// Repo root, resolved from this crate's own manifest directory
/// (`crates/xtask`, two levels below the workspace root) at compile time —
/// this works regardless of the caller's current directory, since `cargo
/// xtask` (the `.cargo/config.toml` alias) is just `cargo run -p xtask`.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn vendor_dir() -> PathBuf {
    repo_root().join("crates/hooks-core/vendor/xahaud-hook")
}

fn src_dir() -> PathBuf {
    repo_root().join("crates/hooks-core/src")
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Generates every target file's *unformatted* content, keyed by its
/// `src/`-relative filename.
fn generate_all() -> Result<BTreeMap<&'static str, String>> {
    let vendor = vendor_dir();
    let error_h = read(&vendor.join("error.h"))?;
    let tts_h = read(&vendor.join("tts.h"))?;
    let ls_flags_h = read(&vendor.join("ls_flags.h"))?;
    let tx_flags_h = read(&vendor.join("tx_flags.h"))?;
    let sfcodes_h = read(&vendor.join("sfcodes.h"))?;
    let hookapi_h = read(&vendor.join("hookapi.h"))?;
    let macro_h = read(&vendor.join("macro.h"))?;
    let extern_h = read(&vendor.join("extern.h"))?;

    let mut out = BTreeMap::new();
    out.insert("error.rs", codegen::error::generate(&error_h)?);
    out.insert("tts.rs", codegen::tts::generate(&tts_h)?);
    out.insert("ls_flags.rs", codegen::ls_flags::generate(&ls_flags_h)?);
    out.insert("tx_flags.rs", codegen::tx_flags::generate(&tx_flags_h)?);
    out.insert("sfcodes.rs", codegen::sfcodes::generate(&sfcodes_h)?);
    out.insert(
        "consts.rs",
        codegen::consts::generate(&hookapi_h, &macro_h)?,
    );
    out.insert("api.rs", codegen::api::generate(&extern_h)?);

    for name in GENERATED_FILES {
        if !out.contains_key(name) {
            bail!("internal error: generator produced no content for {name}");
        }
    }
    Ok(out)
}

/// A scratch directory, auto-removed on drop, carrying a copy of the repo's
/// `rustfmt.toml` so `rustfmt` (run directly, not through `cargo fmt`)
/// discovers the same style config it would inside the real tree.
struct FmtScratch(PathBuf);

impl FmtScratch {
    fn new() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "xtask-gen-core-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let rustfmt_toml = repo_root().join("rustfmt.toml");
        fs::copy(&rustfmt_toml, dir.join("rustfmt.toml"))
            .with_context(|| format!("copying {}", rustfmt_toml.display()))?;
        Ok(Self(dir))
    }

    /// Writes `content` under `filename` in the scratch dir and runs
    /// `rustfmt` on it in place, returning the formatted text.
    fn format(&self, filename: &str, content: &str) -> Result<String> {
        let path = self.0.join(filename);
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;

        // `unsafe extern "C" { ... }` (used in api.rs) requires 2024-edition
        // parsing; `rustfmt` run standalone (not via `cargo fmt`) doesn't
        // infer that from a Cargo.toml, so it's passed explicitly.
        let status = Command::new("rustfmt")
            .args(["--edition", "2024"])
            .arg(&path)
            .status()
            .context("running rustfmt (is it installed? `rustup component add rustfmt`)")?;
        if !status.success() {
            bail!("rustfmt exited with failure formatting {filename}");
        }
        read(&path)
    }
}

impl Drop for FmtScratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn format_all(
    generated: &BTreeMap<&'static str, String>,
) -> Result<BTreeMap<&'static str, String>> {
    let scratch = FmtScratch::new()?;
    let mut formatted = BTreeMap::new();
    for (name, content) in generated {
        formatted.insert(*name, scratch.format(name, content)?);
    }
    Ok(formatted)
}

/// `cargo xtask gen-core`: writes the generated + `rustfmt`-formatted files
/// into `crates/hooks-core/src/`, then runs `cargo fmt -p hooks-core` as a
/// belt-and-braces final pass over the real files.
pub fn run_update() -> Result<()> {
    let generated = generate_all()?;
    let formatted = format_all(&generated)?;

    let dir = src_dir();
    for (name, content) in &formatted {
        let path = dir.join(name);
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }

    let status = Command::new("cargo")
        .args(["fmt", "-p", "hooks-core"])
        .current_dir(repo_root())
        .status()
        .context("running `cargo fmt -p hooks-core`")?;
    if !status.success() {
        bail!("`cargo fmt -p hooks-core` failed");
    }
    Ok(())
}

/// `cargo xtask gen-core --check`: regenerates and formats in a scratch
/// directory and byte-compares the result against
/// `crates/hooks-core/src/*.rs`, without writing anything there. Returns an
/// error naming every mismatched file if any differ (the CI-facing exit-1
/// path); prints a confirmation and returns `Ok(())` when everything
/// matches.
pub fn run_check() -> Result<()> {
    let generated = generate_all()?;
    let formatted = format_all(&generated)?;

    let dir = src_dir();
    let mut mismatched = Vec::new();
    for (name, content) in &formatted {
        let on_disk = read(&dir.join(name)).unwrap_or_default();
        if *content != on_disk {
            mismatched.push(*name);
        }
    }

    if mismatched.is_empty() {
        println!("cargo xtask gen-core --check: crates/hooks-core/src/*.rs is up to date");
        Ok(())
    } else {
        bail!(
            "cargo xtask gen-core --check: out of date: {}\n\
             run `cargo xtask gen-core` and commit the result",
            mismatched.join(", ")
        );
    }
}
