//! Guard, control-flow, and trace macros.
//!
//! `guard!`/`guard_m!` match the C `GUARD`/`GUARDM` macros from `macro.h`
//! exactly, including the `+ 1` on `maxiter`:
//! `GUARD(maxiter)` in C is `_g((1ULL << 31U) + __LINE__, (maxiter) + 1)`.
//! The `unsafe` call to `_g` lives inside the macro expansion, so these are
//! usable from safe code without an `unsafe` block at the call site.
//!
//! `uninit_buf!` is deliberately NOT provided:
//! `MaybeUninit::uninit().assume_init()` for a byte array is UB for this use
//! case. Buffers are always `[0u8; N]`.

/// Guard a loop against exceeding `maxiter` iterations, per the Hook API's
/// static guard-check requirement (see DESIGN.md §2 C2). Matches the C
/// `GUARD` macro's guard-id formula exactly, including the `+ 1` on
/// `maxiter`.
///
/// # Examples
///
/// ```
/// use hooks_lib::guard;
///
/// let mut i = 0;
/// loop {
///     guard!(10);
///     if i >= 3 {
///         break;
///     }
///     i += 1;
/// }
/// assert_eq!(i, 3);
/// ```
#[macro_export]
macro_rules! guard {
    ($m:expr) => {
        unsafe { $crate::raw::_g((1u32 << 31) + line!(), ($m) + 1) }
    };
}

/// Like [`guard!`], but for multiple loops that share one source line (the
/// `$n` disambiguates them). Matches the C `GUARDM` macro's guard-id formula
/// exactly.
#[macro_export]
macro_rules! guard_m {
    ($m:expr, $n:expr) => {
        unsafe { $crate::raw::_g((1u32 << 31) + (line!() << 16) + ($n), ($m) + 1) }
    };
}

/// Terminate hook execution successfully. `accept!()` sends no message and
/// error code `0`; `accept!(msg, code)` forwards both.
///
/// `code` may be a plain `i64` (including untyped integer literals like
/// `-1`) or any value whose type implements `Into<i64>` — e.g. an enum
/// defined with [`hook_errors!`](crate::hook_errors) — since it is passed
/// through `i64::from(code)` before reaching the host call.
#[macro_export]
macro_rules! accept {
    () => {
        $crate::api::control::accept(&[], 0)
    };
    ($msg:expr, $code:expr) => {
        $crate::api::control::accept($msg, i64::from($code))
    };
}

/// Terminate hook execution with a failure, rolling back state changes.
///
/// `code` accepts the same `i64`-literal-or-`Into<i64>` forms as
/// [`accept!`] — see its doc comment for details.
#[macro_export]
macro_rules! rollback {
    ($msg:expr, $code:expr) => {
        $crate::api::control::rollback($msg, i64::from($code))
    };
}

/// Emit a debug trace message. Compiles to nothing unless **hooks-lib's**
/// `trace` feature is enabled (traces cost bytes and execution time) —
/// enable it from a hook crate with
/// `hooks-lib = { ..., features = ["trace"] }`; no feature re-declaration
/// in the hook crate is needed.
#[macro_export]
macro_rules! trace {
    ($msg:expr) => {
        $crate::api::trace::__macro_support::trace_maybe($msg, &[], false)
    };
    ($msg:expr, $data:expr) => {
        $crate::api::trace::__macro_support::trace_maybe($msg, $data, false)
    };
}

/// Emit a debug trace message followed by an integer. Compiles to nothing
/// unless hooks-lib's `trace` feature is enabled (see [`trace!`]).
#[macro_export]
macro_rules! trace_num {
    ($msg:expr, $number:expr) => {
        $crate::api::trace::__macro_support::trace_num_maybe($msg, $number)
    };
}

/// Emit a debug trace message followed by an XFL value. Compiles to nothing
/// unless hooks-lib's `trace` feature is enabled (see [`trace!`]).
#[macro_export]
macro_rules! trace_float {
    ($msg:expr, $value:expr) => {
        $crate::api::trace::__macro_support::trace_float_maybe($msg, $value)
    };
}

/// Compile-time zero-padding helper backing [`pad!`](crate::pad).
///
/// Copies `src` into the start of a zeroed `[u8; N]`. The `pad!` macro wraps
/// every call in an inline `const` block, so the `assert!` and the indexing
/// below are compile-time checks — they can never become runtime panics.
#[doc(hidden)]
#[allow(clippy::indexing_slicing)] // in-bounds by the assert, const-evaluated only
pub const fn padded_bytes<const N: usize>(src: &[u8]) -> [u8; N] {
    assert!(
        src.len() <= N,
        "pad!: source is larger than the destination"
    );

    let mut output = [0u8; N];
    let mut i = 0;

    while i < src.len() {
        output[i] = src[i];
        i = i.wrapping_add(1);
    }

    output
}

/// Zero-pad a constant byte string to a fixed-size array, at compile time.
///
/// The array length is inferred from the surrounding context. The expansion
/// is an inline `const` block, which has two consequences:
///
/// - the argument must be a constant expression (e.g. a byte-string
///   literal), and a source longer than the destination is a **compile
///   error**, never a runtime panic;
/// - the padded array is baked into the binary at compile time — no copy or
///   zeroing loop exists at runtime, so no loop guard is needed for it.
///
/// The canonical use is building fixed-size state keys and namespaces from
/// short names.
///
/// This right-pads (`src` at the front, zero bytes at the end) — this
/// crate's own convention for building a `StateKey`/`NameSpace`-shaped
/// constant. For the host's own convention when a short key is passed
/// directly (as a C hook does) — the host **left**-pads it instead — see
/// [`pad_left!`](crate::pad_left) and DESIGN.md §5.6 ("Endianness
/// conventions").
///
/// # Examples
/// ```
/// use hooks_lib::pad;
/// use hooks_lib::types::StateKey;
///
/// let padded: [u8; 10] = pad!(b"hello");
/// assert_eq!(padded, [b'h', b'e', b'l', b'l', b'o', 0, 0, 0, 0, 0]);
///
/// // `StateKey` is a `#[repr(transparent)]` newtype over `[u8; 32]` (see
/// // `types.rs`), so `pad!` (which returns a plain `[u8; N]`) is wrapped
/// // explicitly via the newtype's public tuple field.
/// const KEY: StateKey = StateKey(pad!(b"counter"));
/// assert!(KEY.starts_with(b"counter"));
/// ```
///
/// A source longer than the destination fails to compile:
/// ```compile_fail
/// let too_small: [u8; 4] = hooks_lib::pad!(b"hello");
/// ```
#[macro_export]
macro_rules! pad {
    ($value:expr) => {
        const { $crate::padded_bytes($value) }
    };
}

/// Compile-time zero-padding helper backing [`pad_left!`](crate::pad_left).
///
/// Copies `src` into the *end* of a zeroed `[u8; N]` — the mirror image of
/// [`padded_bytes`], which copies `src` into the start. The `pad_left!`
/// macro wraps every call in an inline `const` block, so the `assert!` and
/// the indexing below are compile-time checks — they can never become
/// runtime panics.
#[doc(hidden)]
#[allow(clippy::indexing_slicing)] // in-bounds by the assert, const-evaluated only
pub const fn padded_bytes_left<const N: usize>(src: &[u8]) -> [u8; N] {
    assert!(
        src.len() <= N,
        "pad_left!: source is larger than the destination"
    );

    let mut output = [0u8; N];
    let offset = N.wrapping_sub(src.len());
    let mut i = 0;

    while i < src.len() {
        output[offset.wrapping_add(i)] = src[i];
        i = i.wrapping_add(1);
    }

    output
}

/// Zero-pad a constant byte string to a fixed-size array, at compile time —
/// on the **left** (zero bytes first, `src` at the end), the mirror image
/// of [`pad!`](crate::pad).
///
/// The host itself left-pads a state/param key shorter than the fixed key
/// width (32 bytes for hook state, 1–32 bytes for hook/otxn parameters) —
/// see DESIGN.md §5.6 ("Endianness conventions"). Reach for `pad_left!`,
/// not [`pad!`](crate::pad), when a Rust hook needs to address the exact
/// same state slot a C hook reaches by passing a short raw key: `pad!`
/// building the same short name instead would produce a *different*
/// 32-byte key (value at the front, not the end), pointing at a different
/// state entry.
///
/// Same compile-time-only shape as [`pad!`](crate::pad): the array length
/// is inferred from context, the argument must be a constant expression, a
/// source longer than the destination is a compile error, and the padded
/// array is baked into the binary at compile time (no runtime copy/zeroing
/// loop, so no loop guard is needed for it).
///
/// # Examples
/// ```
/// use hooks_lib::pad_left;
/// use hooks_lib::types::StateKey;
///
/// let padded: [u8; 10] = pad_left!(b"hello");
/// assert_eq!(padded, [0, 0, 0, 0, 0, b'h', b'e', b'l', b'l', b'o']);
///
/// // The same short C-hook-style key, reproduced so a Rust hook lands on
/// // the exact same state slot.
/// const KEY: StateKey = StateKey(pad_left!(b"counter"));
/// assert!(KEY.ends_with(b"counter"));
/// ```
///
/// A source longer than the destination fails to compile:
/// ```compile_fail
/// let too_small: [u8; 4] = hooks_lib::pad_left!(b"hello");
/// ```
#[macro_export]
macro_rules! pad_left {
    ($value:expr) => {
        const { $crate::padded_bytes_left($value) }
    };
}
