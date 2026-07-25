//! `hooks-build` CLI: drives `cargo build --target wasm32v1-none`, then
//! post-processes and validates the resulting wasm into a SetHook-legal
//! Hook binary. See `docs/DESIGN.md` §6.1 for the full command reference.

use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hooks_build::{ApiVersion, Options, ValidationReport};

/// A CLI toolchain for building and validating Xahau Hook wasm binaries.
#[derive(Parser)]
#[command(name = "hooks-build", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Builds a Rust crate for `wasm32v1-none`, then cleans and validates
    /// the result into a SetHook-legal binary.
    Build {
        /// Path to the crate's `Cargo.toml` (forwarded to `cargo build`).
        #[arg(long)]
        manifest_path: Option<PathBuf>,
        /// Build only the named package (forwarded to `cargo build -p`).
        #[arg(short = 'p', long)]
        package: Option<String>,
        /// The Hook API version this module targets.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=1))]
        api_version: u8,
        /// Insert missing loop guards instead of treating them as an error.
        #[arg(long)]
        auto_guard: bool,
        /// `maxiter` used for auto-inserted guards.
        #[arg(long, default_value_t = 16)]
        default_maxiter: u32,
        /// Directory to write the output binary to (default: `out/` next to
        /// the manifest).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Write the output even if it exceeds the 65,535-byte SetHook
        /// limit (clearly marked invalid).
        #[arg(long)]
        allow_oversize: bool,
        /// Run `wasm-opt -Oz` between the cleaner and the flatten pass
        /// (vendored via the `wasm-opt` crate — no system install
        /// required). Every later stage still re-validates the result in
        /// full.
        #[arg(long)]
        optimize: bool,
    },
    /// Cleans and validates an already-built wasm file, without invoking
    /// cargo.
    Clean {
        /// The input wasm file.
        input: PathBuf,
        /// Where to write the cleaned binary (default:
        /// `<input>.clean.wasm`).
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
        /// The Hook API version this module targets.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=1))]
        api_version: u8,
        /// Insert missing loop guards instead of treating them as an error.
        #[arg(long)]
        auto_guard: bool,
        /// `maxiter` used for auto-inserted guards.
        #[arg(long, default_value_t = 16)]
        default_maxiter: u32,
        /// Write the output even if it exceeds the 65,535-byte SetHook
        /// limit (clearly marked invalid).
        #[arg(long)]
        allow_oversize: bool,
        /// Run `wasm-opt -Oz` between the cleaner and the flatten pass
        /// (vendored via the `wasm-opt` crate — no system install
        /// required). Every later stage still re-validates the result in
        /// full.
        #[arg(long)]
        optimize: bool,
    },
    /// Validates a wasm file against the full SetHook rule set, without
    /// modifying it. Works on any wasm, including C-built hooks.
    Check {
        /// The wasm file to validate.
        file: PathBuf,
        /// The Hook API version this module targets.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=1))]
        api_version: u8,
    },
}

fn api_version_from(v: u8) -> ApiVersion {
    if v == 1 {
        ApiVersion::V1
    } else {
        ApiVersion::V0
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Build {
            manifest_path,
            package,
            api_version,
            auto_guard,
            default_maxiter,
            out,
            allow_oversize,
            optimize,
        } => {
            let opts = Options {
                api_version: api_version_from(api_version),
                auto_guard,
                default_maxiter,
                allow_oversize,
                optimize,
            };
            cmd_build(manifest_path, package, out, &opts)
        }
        Cmd::Clean {
            input,
            out,
            api_version,
            auto_guard,
            default_maxiter,
            allow_oversize,
            optimize,
        } => {
            let opts = Options {
                api_version: api_version_from(api_version),
                auto_guard,
                default_maxiter,
                allow_oversize,
                optimize,
            };
            cmd_clean(&input, out, &opts)
        }
        Cmd::Check { file, api_version } => {
            let opts = Options {
                api_version: api_version_from(api_version),
                ..Options::default()
            };
            cmd_check(&file, &opts)
        }
    }
}

fn print_report(report: &ValidationReport) {
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    if let Some(verdict) = report.guard_verdict {
        println!(
            "worst-case instructions: hook={} cbak={}",
            verdict.hook_cost, verdict.cbak_cost
        );
    }
    println!("max nesting depth: {}", report.max_nesting_depth);
}

fn print_size_and_fee(bytes: &[u8]) {
    let fee = hooks_build::estimate_fee(bytes.len());
    println!("size: {} bytes", fee.bytes);
    println!(
        "estimated SetHook fee: {} drops ({} XAH)",
        fee.drops,
        fee.xah_string()
    );
}

/// Prints the `--optimize` before/after comparison: `docs/DESIGN.md` M7 (see
/// `hooks_build::OptimizeReport`).
///
/// `arithmetic_side_effects` is allowed locally: `before_bytes`/`after_bytes`
/// are both SetHook wasm sizes (bounded by the 65,535-byte SetHook limit —
/// nowhere near `i64` overflow), so this is display-only bookkeeping, not a
/// guest-facing failure mode (see the crate-level rationale in
/// `hooks-build/src/lib.rs`).
#[allow(clippy::arithmetic_side_effects)]
fn print_optimize_summary(summary: &hooks_build::OptimizeReport) {
    let delta_bytes = summary.after_bytes as i64 - summary.before_bytes as i64;
    let pct = if summary.before_bytes == 0 {
        0.0
    } else {
        (delta_bytes as f64 / summary.before_bytes as f64) * 100.0
    };
    println!("--optimize (wasm-opt -Oz):");
    println!(
        "  size: {} -> {} bytes ({delta_bytes:+} bytes, {pct:+.1}%)",
        summary.before_bytes, summary.after_bytes
    );
    println!(
        "  estimated SetHook fee: {} -> {} drops ({} -> {} XAH)",
        summary.before_fee.drops,
        summary.after_fee.drops,
        summary.before_fee.xah_string(),
        summary.after_fee.xah_string()
    );
    match (summary.before_guard, summary.after_guard) {
        (Some(before), Some(after)) => println!(
            "  guard worst-case instructions: hook {} -> {}, cbak {} -> {}",
            before.hook_cost, after.hook_cost, before.cbak_cost, after.cbak_cost
        ),
        _ => println!(
            "  guard worst-case instructions: unavailable (api-version 1, or the guard \
             checker could not run on one of the two shapes)"
        ),
    }
}

fn cmd_build(
    manifest_path: Option<PathBuf>,
    package: Option<String>,
    out: Option<PathBuf>,
    opts: &Options,
) -> Result<()> {
    let cargo = find_cargo()?;

    let mut cmd = Command::new(&cargo);
    cmd.args([
        "build",
        "--release",
        "--target",
        "wasm32v1-none",
        "--message-format=json-render-diagnostics",
    ]);
    if let Some(mp) = &manifest_path {
        cmd.arg("--manifest-path").arg(mp);
    }
    if let Some(p) = &package {
        cmd.args(["-p", p]);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn `{}`", cargo.display()))?;
    let stdout = child
        .stdout
        .take()
        .context("internal error: cargo's stdout was not piped")?;
    let reader = std::io::BufReader::new(stdout);

    let mut artifact: Option<PathBuf> = None;
    for line in reader.lines() {
        let line = line.context("reading cargo output")?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // cargo can emit non-JSON lines on some setups; ignore
        };
        if msg.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
            && let Some(filenames) = msg.get("filenames").and_then(|f| f.as_array())
        {
            for f in filenames {
                if let Some(s) = f.as_str()
                    && s.ends_with(".wasm")
                {
                    artifact = Some(PathBuf::from(s));
                }
            }
        }
    }

    let status = child.wait().context("waiting for cargo build")?;
    if !status.success() {
        bail!("cargo build failed ({status})");
    }
    let artifact = artifact.context(
        "cargo build did not produce a .wasm artifact; is this a `cdylib` crate for wasm32v1-none?",
    )?;

    let wasm = std::fs::read(&artifact)
        .with_context(|| format!("reading build artifact {}", artifact.display()))?;

    let out_dir = out.unwrap_or_else(|| default_out_dir(manifest_path.as_deref()));
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("creating output directory {}", out_dir.display()))?;
    let file_name = artifact
        .file_name()
        .context("build artifact has no file name")?;
    let out_path = out_dir.join(file_name);

    run_and_write(&wasm, &out_path, opts)
}

fn cmd_clean(input: &Path, out: Option<PathBuf>, opts: &Options) -> Result<()> {
    let wasm = std::fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let out_path = out.unwrap_or_else(|| {
        let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        input.with_file_name(format!("{stem}.clean.wasm"))
    });
    run_and_write(&wasm, &out_path, opts)
}

fn run_and_write(wasm: &[u8], out_path: &Path, opts: &Options) -> Result<()> {
    let (output, report) = hooks_build::run_pipeline(wasm, opts)?;
    print_report(&report);
    if let Some(summary) = &report.optimize_report {
        print_optimize_summary(summary);
    }
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let mut f = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    f.write_all(&output)
        .with_context(|| format!("writing {}", out_path.display()))?;
    println!("wrote {}", out_path.display());
    print_size_and_fee(&output);
    if report.oversize_allowed {
        println!("WARNING: output marked INVALID (oversize)");
    }
    Ok(())
}

fn cmd_check(file: &Path, opts: &Options) -> Result<()> {
    let wasm = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    match hooks_build::verify(&wasm, opts) {
        Ok(report) => {
            print_report(&report);
            println!("OK: {} is a valid SetHook wasm binary", file.display());
            print_size_and_fee(&wasm);
            Ok(())
        }
        Err(e) => {
            eprintln!("INVALID: {} failed validation:", file.display());
            for line in e.to_string().lines() {
                eprintln!("  - {line}");
            }
            std::process::exit(1);
        }
    }
}

fn default_out_dir(manifest_path: Option<&Path>) -> PathBuf {
    let manifest_dir = manifest_path
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    manifest_dir.join("out")
}

/// Locates the `cargo` executable to invoke for `build`. The user's shell
/// PATH is inherited verbatim (the process environment is never cleared),
/// so this just needs to find *a* `cargo` on it — the same one the user's
/// shell would run.
fn find_cargo() -> Result<PathBuf> {
    if let Ok(cargo) = std::env::var("CARGO") {
        return Ok(PathBuf::from(cargo));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("cargo");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!(
        "could not find `cargo` on PATH; run `hooks-build build` from a shell where \
         `cargo build` already works"
    )
}
