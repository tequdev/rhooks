//! Orchestrates `cargo xtask gen-core`: reads the vendored xahaud headers
//! (`crates/hooks-core/vendor/xahaud-hook/`), parses them once into a single
//! [`crate::ir::HookApiSpec`], round-trips that spec through
//! `crates/hooks-core/hook_api.json`, runs each per-file generator in
//! [`crate::codegen`] against the round-tripped spec, formats the output
//! with `rustfmt` under the repo's `rustfmt.toml`, and either writes the
//! result into `crates/hooks-core/` or (`--check`) compares it against
//! what's already there without touching the working tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::codegen;
use crate::ir::{self, HookApiSpec};

/// The set of `src/`-relative `.rs` files this generator owns. `lib.rs` is
/// deliberately excluded (`docs/DESIGN.md` §4): it's hand-wired
/// module/re-export plumbing, not a header translation, and the spec calls
/// it out as NOT generated.
const GENERATED_FILES: &[&str] = &[
    "error.rs",
    "tts.rs",
    "ls_flags.rs",
    "tx_flags.rs",
    "sfcodes.rs",
    "consts.rs",
    "api.rs",
    "host.rs",
];

/// The generated intermediate-representation file, checked in at the
/// `hooks-core` crate root (not under `src/`, since it isn't Rust source):
/// the pipeline's `hook_api.json` artifact (module docs on [`crate::ir`]).
const HOOK_API_JSON: &str = "hook_api.json";

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

/// `crates/hooks-core`'s crate root — where `hook_api.json` lives, one level
/// above `src/`.
fn crate_dir() -> PathBuf {
    repo_root().join("crates/hooks-core")
}

fn src_dir() -> PathBuf {
    crate_dir().join("src")
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Parses the eight vendored headers into a [`HookApiSpec`] and renders it
/// as pretty-printed, canonical JSON (trailing newline, no ambiguity in
/// key/array order — struct field order is derive-stable, and every
/// sequence here is already in header order).
fn build_hook_api_json() -> Result<String> {
    let vendor = vendor_dir();
    let error_h = read(&vendor.join("error.h"))?;
    let tts_h = read(&vendor.join("tts.h"))?;
    let ls_flags_h = read(&vendor.join("ls_flags.h"))?;
    let tx_flags_h = read(&vendor.join("tx_flags.h"))?;
    let sfcodes_h = read(&vendor.join("sfcodes.h"))?;
    let hookapi_h = read(&vendor.join("hookapi.h"))?;
    let macro_h = read(&vendor.join("macro.h"))?;
    let extern_h = read(&vendor.join("extern.h"))?;

    let spec = ir::build(
        &error_h,
        &tts_h,
        &ls_flags_h,
        &tx_flags_h,
        &sfcodes_h,
        &hookapi_h,
        &macro_h,
        &extern_h,
    )?;
    let mut json = serde_json::to_string_pretty(&spec).context("serializing HookApiSpec")?;
    json.push('\n');
    Ok(json)
}

/// Generates every target `.rs` file's *unformatted* content, keyed by its
/// `src/`-relative filename, from `hook_api_json` — the same JSON text that
/// gets written to (or checked against) `crates/hooks-core/hook_api.json`.
/// The JSON is deserialized back into a [`HookApiSpec`] here (rather than
/// reusing the in-memory spec that produced it) so every generator
/// genuinely consumes the intermediate representation, not the parser's
/// output directly.
fn generate_rust_files(hook_api_json: &str) -> Result<BTreeMap<&'static str, String>> {
    let spec: HookApiSpec =
        serde_json::from_str(hook_api_json).context("deserializing hook_api.json")?;

    let mut out = BTreeMap::new();
    out.insert("error.rs", codegen::error::generate(&spec.error_codes)?);
    out.insert("tts.rs", codegen::tts::generate(&spec.tts)?);
    out.insert("ls_flags.rs", codegen::ls_flags::generate(&spec.ls_flags)?);
    out.insert("tx_flags.rs", codegen::tx_flags::generate(&spec.tx_flags)?);
    out.insert("sfcodes.rs", codegen::sfcodes::generate(&spec.sfcodes)?);
    out.insert(
        "consts.rs",
        codegen::consts::generate(
            &spec.keylet,
            &spec.compare,
            &spec.canonical,
            &spec.at_family,
            &spec.am_family,
        )?,
    );
    out.insert("api.rs", codegen::api::generate(&spec.functions)?);
    out.insert("host.rs", codegen::host::generate(&spec.functions)?);

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

/// `cargo xtask gen-core`: writes `hook_api.json`, then the generated +
/// `rustfmt`-formatted `.rs` files, into `crates/hooks-core/`, then runs
/// `cargo fmt -p hooks-core` as a belt-and-braces final pass over the real
/// files.
pub fn run_update() -> Result<()> {
    let hook_api_json = build_hook_api_json()?;
    let generated = generate_rust_files(&hook_api_json)?;
    let formatted = format_all(&generated)?;

    let json_path = crate_dir().join(HOOK_API_JSON);
    fs::write(&json_path, &hook_api_json)
        .with_context(|| format!("writing {}", json_path.display()))?;
    println!("wrote {}", json_path.display());

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

/// `cargo xtask gen-core --check`: regenerates `hook_api.json` and formats
/// the `.rs` files in a scratch directory, then byte-compares both against
/// `crates/hooks-core/hook_api.json` and `crates/hooks-core/src/*.rs`,
/// without writing anything there. Returns an error naming every mismatched
/// file if any differ (the CI-facing exit-1 path); prints a confirmation and
/// returns `Ok(())` when everything matches.
pub fn run_check() -> Result<()> {
    let hook_api_json = build_hook_api_json()?;
    let generated = generate_rust_files(&hook_api_json)?;
    let formatted = format_all(&generated)?;

    let mut mismatched = Vec::new();

    let json_on_disk = read(&crate_dir().join(HOOK_API_JSON)).unwrap_or_default();
    if hook_api_json != json_on_disk {
        mismatched.push(HOOK_API_JSON);
    }

    let dir = src_dir();
    for (name, content) in &formatted {
        let on_disk = read(&dir.join(name)).unwrap_or_default();
        if *content != on_disk {
            mismatched.push(*name);
        }
    }

    if mismatched.is_empty() {
        println!(
            "cargo xtask gen-core --check: crates/hooks-core/hook_api.json and src/*.rs are up to date"
        );
        Ok(())
    } else {
        bail!(
            "cargo xtask gen-core --check: out of date: {}\n\
             run `cargo xtask gen-core` and commit the result",
            mismatched.join(", ")
        );
    }
}
