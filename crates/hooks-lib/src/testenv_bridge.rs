//! Internal glue for routing wrapper functions in `api/*.rs` to a
//! `hooks-core` testenv backend, when one is installed.
//!
//! Compiled only off `wasm32` and behind the `testenv` feature — see
//! `crates/hooks-core/src/backend.rs`'s module doc for the full rationale
//! (in short: this crate's wrapper functions hold real `&[u8]`/`&mut [u8]`
//! slices at the point they'd otherwise cast a pointer down to `u32` for
//! the `wasm32`-mirroring FFI call, so routing to the backend happens here,
//! one layer above `hooks-core`, instead of inside its raw stubs).
//!
//! Not part of the public API surface; every item here is `pub(crate)`.

extern crate std;

use crate::error::{HookError, Result};

/// Copy as much of `src` into `out` as fits, returning the number of bytes
/// copied. Mirrors the raw Hook API's own caller-buffer contract: truncate
/// silently, report how much was actually written.
#[inline(always)]
pub(crate) fn copy_into(out: &mut [u8], src: &[u8]) -> usize {
    let n = out.len().min(src.len());
    match (out.get_mut(..n), src.get(..n)) {
        (Some(dst), Some(s)) => {
            dst.copy_from_slice(s);
            n
        }
        // Unreachable in practice (`n` is bounded by both lengths above),
        // but panic-free regardless.
        _ => 0,
    }
}

/// Adapt a backend byte-buffer result (`Ok(bytes)` / `Err(code)`) into this
/// crate's `(bytes written, HookError)` convention, copying into `out`.
#[inline(always)]
pub(crate) fn write_bytes(
    out: &mut [u8],
    result: std::result::Result<std::vec::Vec<u8>, i64>,
) -> Result<usize> {
    match result {
        Ok(bytes) => Ok(copy_into(out, &bytes)),
        Err(code) => Err(HookError::from(code)),
    }
}
