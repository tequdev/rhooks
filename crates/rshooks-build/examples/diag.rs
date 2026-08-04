//! Throwaway diagnostic: dump intermediate pipeline artifacts for manual
//! inspection. Not part of the shipped tool.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let input = &args[1];
    let wasm = fs::read(input).unwrap();
    let opts = rshooks_build::Options::default();
    let cleaned = rshooks_build::clean(&wasm, &opts).unwrap();
    fs::write("/tmp/diag_cleaned.wasm", &cleaned).unwrap();
    let (flattened, freport) = rshooks_build::flatten(&cleaned).unwrap();
    for n in &freport.notes {
        eprintln!("flatten note: {n}");
    }
    fs::write("/tmp/diag_flattened.wasm", &flattened).unwrap();
    let (unnested, ureport) = rshooks_build::unnest(&flattened).unwrap();
    for n in &ureport.notes {
        eprintln!("unnest note: {n}");
    }
    fs::write("/tmp/diag_unnested.wasm", &unnested).unwrap();
    println!("wrote /tmp/diag_{{cleaned,flattened,unnested}}.wasm");
}
