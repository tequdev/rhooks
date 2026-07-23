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
#[macro_export]
macro_rules! accept {
    () => {
        $crate::api::control::accept(&[], 0)
    };
    ($msg:expr, $code:expr) => {
        $crate::api::control::accept($msg, $code)
    };
}

/// Terminate hook execution with a failure, rolling back state changes.
#[macro_export]
macro_rules! rollback {
    ($msg:expr, $code:expr) => {
        $crate::api::control::rollback($msg, $code)
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
