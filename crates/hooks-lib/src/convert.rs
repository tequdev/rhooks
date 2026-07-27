//! Boundary conversion traits: [`ToBytes`]/[`FromBytes`].
//!
//! These traits fix exactly how a small, fixed-size Rust value crosses the
//! boundary into/out of a protocol byte buffer (a hook state entry, a
//! `state_keys!`-encoded key payload, ...). They generalize the "little-
//! endian, fixed layout" convention this crate already documents for
//! [`crate::api::state::state_u64`]'s *underlying state entries* (as
//! opposed to that function's own big-endian "as-int64" wire encoding —
//! see its doc comment) so the typed storage layer (`crate::state`'s
//! `state_get`/`state_set_typed`/`state_update_typed`) can encode/decode
//! arbitrary fixed-size types without repeating that logic per call site.
//!
//! # Implementor's contract
//!
//! Every impl of [`ToBytes`]/[`FromBytes`] — the ones in this module, the
//! newtype impls in `types.rs`, and any a hook crate adds for its own
//! types — must stay panic-free, loop-free, and heap-free, the same
//! constraints as every other hooks-lib wrapper (DESIGN.md §2 C2/C7):
//! `wasm32v1-none` hook binaries have no allocator, and an unguarded loop
//! fails the Hook API's guard checker. Concretely:
//!
//! - Never index with `buf[i]` (this crate denies `clippy::indexing_slicing`
//!   crate-wide) — use `.get()`/`.get_mut()` over a range whose bounds are
//!   compile-time constants, then `copy_from_slice`. Keeping the range
//!   compile-time-constant (never a runtime-computed length) is what keeps
//!   the copy a handful of inlined loads/stores instead of a lowering to a
//!   `memcpy`/`memcmp` call with a runtime length — exactly the std idiom
//!   DESIGN.md warns produces unguardable loops in a Hook binary.
//! - [`ToBytes::MAX_LEN`] must be a compile-time constant equal to the
//!   exact number of bytes a successful [`ToBytes::write`] produces.
//! - [`ToBytes::write`] must not panic if `buf` is shorter than `MAX_LEN`:
//!   write nothing and return `0` instead (mirrors this crate's other
//!   caller-buffer wrappers, which rely on the host's own bounds checking
//!   rather than panicking locally).

use crate::error::{HookError, Result};

/// Encode `Self` into the front of a caller-provided buffer.
///
/// Mirrors this crate's caller-buffer convention (`state`, `hook_account`,
/// ...): implementations never allocate and never panic. See the module
/// doc comment for the loop-free/panic-free contract every impl must
/// uphold.
pub trait ToBytes {
    /// The exact number of bytes a successful [`ToBytes::write`] produces.
    const MAX_LEN: usize;

    /// Write `self`'s encoding into `buf[..Self::MAX_LEN]`.
    ///
    /// Returns `Self::MAX_LEN` (the number of bytes written) on success, or
    /// `0` if `buf` is shorter than `Self::MAX_LEN` (nothing is written in
    /// that case — never a partial write).
    fn write(&self, buf: &mut [u8]) -> usize;
}

/// Decode `Self` from a byte buffer.
pub trait FromBytes: Sized {
    /// Decode `Self` from `buf`.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::TooSmall`] if `buf` is shorter than the
    /// encoding this type expects.
    fn read(buf: &[u8]) -> Result<Self>;
}

impl ToBytes for u8 {
    const MAX_LEN: usize = 1;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..1) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                1
            }
            None => 0,
        }
    }
}

impl FromBytes for u8 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..1).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 1];
        arr.copy_from_slice(src);
        Ok(u8::from_le_bytes(arr))
    }
}

impl ToBytes for u16 {
    const MAX_LEN: usize = 2;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..2) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                2
            }
            None => 0,
        }
    }
}

impl FromBytes for u16 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..2).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 2];
        arr.copy_from_slice(src);
        Ok(u16::from_le_bytes(arr))
    }
}

impl ToBytes for u32 {
    const MAX_LEN: usize = 4;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..4) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                4
            }
            None => 0,
        }
    }
}

impl FromBytes for u32 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..4).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 4];
        arr.copy_from_slice(src);
        Ok(u32::from_le_bytes(arr))
    }
}

impl ToBytes for u64 {
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..8) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                8
            }
            None => 0,
        }
    }
}

impl FromBytes for u64 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..8).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(src);
        Ok(u64::from_le_bytes(arr))
    }
}

impl ToBytes for i64 {
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..8) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                8
            }
            None => 0,
        }
    }
}

impl FromBytes for i64 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..8).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(src);
        Ok(i64::from_le_bytes(arr))
    }
}

impl ToBytes for crate::xfl::XFL {
    // An XFL is an opaque wrapper over a raw `i64` bit pattern (see
    // `xfl.rs`), so it shares `i64`'s width and little-endian convention.
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        self.raw_bits().write(buf)
    }
}

impl FromBytes for crate::xfl::XFL {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        i64::read(buf).map(crate::xfl::XFL::from_raw_bits)
    }
}

impl<const N: usize> ToBytes for [u8; N] {
    const MAX_LEN: usize = N;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..N) {
            Some(dst) => {
                dst.copy_from_slice(self);
                N
            }
            None => 0,
        }
    }
}

impl<const N: usize> FromBytes for [u8; N] {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..N).ok_or(HookError::TooSmall)?;
        let mut out = [0u8; N];
        out.copy_from_slice(src);
        Ok(out)
    }
}

/// A type whose value can be read in one shot as a fixed-size buffer from a
/// caller-buffer Hook API wrapper (`otxn_field`, `hook_param`, `slot`,
/// `state`) — the trait backing
/// [`crate::api::otxn::otxn_field_exact`]/[`crate::api::hook_ctx::hook_param_exact`]/
/// [`crate::api::slot::slot_exact`]/[`crate::api::state::state_exact`].
///
/// Implemented here for `[u8; N]` (any `N`), and — via the
/// `fixed_bytes_type!` macro — in `types.rs` for every `hooks_lib::types`
/// newtype. Each impl knows its *own* exact length, so
/// `otxn_field_exact::<AccountId>(sfAccount)` reads exactly `ACC_ID_LEN`
/// bytes without a caller-supplied `N`; with the return type inferred from
/// a `let` binding's type annotation instead of a turbofish
/// (`let sender: AccountId = otxn_field_exact(sfAccount)?;`), no turbofish
/// is needed at all. A binding with no inferable type (no annotation, no
/// other usage that pins the type down) is a compile error, same as any
/// other unconstrained generic return type — annotate the binding.
///
/// # Why `read_exact` takes a closure, not a length
///
/// A single generic function can't allocate `[0u8; T::SOME_ASSOCIATED_LEN]`
/// — using an associated constant as an array length needs
/// `generic_const_exprs`, unstable on this crate's pinned stable toolchain.
/// `read_exact` sidesteps that by moving the actual `[0u8; N]`/newtype
/// buffer allocation into *this trait method's own implementation*, one
/// per concrete `Self`, where the length is a literal the `impl` block
/// already knows — never a generic parameter's associated constant. The
/// caller-buffer wrapper function itself (`otxn_field`, `state`, ...) is
/// passed in as a closure, so each `*_exact` function stays a single
/// generic function (one per Hook API call), not one monomorphized-away
/// non-generic function per concrete `T` written by hand.
pub trait FixedRead: Sized {
    /// Allocates this type's own fixed-size buffer, calls `read` with it,
    /// and returns `Self` if `read` reports writing exactly that many
    /// bytes.
    ///
    /// # Errors
    ///
    /// Propagates any error `read` returns. Returns
    /// [`HookError::TooSmall`] if `read` succeeds but reports writing a
    /// different number of bytes than this type's exact length (covers
    /// both "wrote fewer bytes" — the common case, e.g. no such field — and
    /// "wrote more," which `read`'s exactly-sized buffer argument should
    /// already prevent at the host level, but isn't assumed here).
    fn read_exact(read: impl FnOnce(&mut [u8]) -> Result<usize>) -> Result<Self>;
}

impl<const N: usize> FixedRead for [u8; N] {
    #[inline(always)]
    fn read_exact(read: impl FnOnce(&mut [u8]) -> Result<usize>) -> Result<Self> {
        let mut out = [0u8; N];
        let written = read(&mut out)?;
        if written == N {
            Ok(out)
        } else {
            Err(HookError::TooSmall)
        }
    }
}

/// Maximum length, in bytes, of a `hook_param`/`otxn_param` parameter
/// *name* — the Hook API's own bound on the `read_len` argument naming the
/// parameter (`hook_api.h`: `TOO_BIG` above 32 bytes, `TOO_SMALL` below 1).
///
/// A parameter name is **not** a fixed-32-byte, zero-padded key the way a
/// [`crate::types::StateKey`] is (see [`crate::state::StateKeyEncode`]) —
/// it is matched at its own *natural* length: `"MIN"` (3 bytes) and a
/// hypothetical zero-padded 32-byte version of the same three bytes name
/// two different parameters, not one. [`ParamName`]'s encoding reflects
/// that: a name's wire bytes are exactly its [`ToBytes::MAX_LEN`], never
/// padded up to this constant — this constant is only an upper (and, at
/// `1`, a lower) bound the encoded length must satisfy.
pub const PARAM_NAME_MAX_LEN: usize = 32;

/// A [`FixedRead`] type that names its own `hook_param`/`otxn_param`
/// parameter.
///
/// [`crate::api::hook_ctx::hook_param_exact`]/
/// [`crate::api::otxn::otxn_param_exact`] take the parameter's name and the
/// value type `T` as two *independent* arguments — nothing stops calling
/// `otxn_param_exact::<WrongType>(b"INS")` for a name/type pairing that was
/// never intended, as long as `WrongType: FixedRead` (true of nearly every
/// fixed-size type this crate provides). Implementing `ParamName` for a
/// type (via [`param_name!`](crate::param_name)) ties it to exactly one
/// parameter name; [`crate::api::hook_ctx::hook_param_kv`]/
/// [`crate::api::otxn::otxn_param_kv`] then read the name off the type
/// itself — there is no second, independently-spelled name argument left
/// for a mismatch to hide in.
///
/// A Hook API parameter name is a genuine **variable-length key of up to
/// [`PARAM_NAME_MAX_LEN`] (32) bytes** — the same shape as a hook state key
/// (see [`crate::state::StateKeyEncode`]) — so [`Name`](Self::Name) may be
/// any [`ToBytes`] type, not just a raw byte string: a whole composite
/// [`crate::HookData`] struct works exactly like a composite state key,
/// with the one difference that a parameter name is encoded at its own
/// **natural** length, never zero-padded to a fixed size.
///
/// # Zero-cost for the (overwhelmingly common) plain-byte-string case
///
/// Encoding an arbitrary `ToBytes` value into bytes is, in general, a real
/// (if small) runtime computation — [`name_bytes`](Self::name_bytes)'s
/// default body runs `Self::NAME.write(..)` once per call (Rust has no
/// stable way to run a trait method at compile time yet). But when
/// `Self::Name` is already a plain `[u8; N]` byte array — the overwhelming
/// common case, e.g. `b"CFG"` — its wire encoding *is* its in-memory
/// representation, with nothing to compute: [`param_name!`](crate::param_name)'s
/// simple form overrides `name_bytes` to return a direct, `'static`
/// reference to `Self::NAME` (a compile-time constant, promoted to static
/// storage — no copy, no buffer, nothing to encode), skipping the default
/// body entirely. `examples/12_typed-data`'s README measures this
/// directly: its plain `CFG`/`INS` names cost the identical worst-case
/// instruction count as the loose `hook_param_exact`/`otxn_param_exact`
/// this replaces.
///
/// # Relationship to the hook-state typed layer
///
/// `ParamName` deliberately mirrors [`crate::state::TypedStateKey`] (see
/// its doc comment for the full comparison table): declare the pairing
/// once ([`param_name!`](crate::param_name) here,
/// [`crate::state_key_value!`] there), then call an accessor
/// (`hook_param_kv`/`otxn_param_kv` here, `state_get_kv`/`state_set_kv`/
/// `state_update_kv` there) that resolves the paired type from the
/// key/name itself — no turbofish, no chance of a mismatch. The
/// difference: a Hook API parameter is normally one compile-time-known
/// name per type (not a key with many runtime instances like a state
/// key), so `Self` here plays both the "key" (`Self::NAME`) and "value"
/// (`Self`, via [`FixedRead`]) roles that state splits into two separate
/// types (`K` and `K::Value`); and a parameter is read-only from the
/// reading hook's own perspective, so there is no `hook_param`/
/// `otxn_param` counterpart to `state_set_kv`/`state_update_kv`.
pub trait ParamName: FixedRead {
    /// The type this parameter's name is encoded from — `[u8; N]` for a
    /// plain byte-string name (the common case — see
    /// [`param_name!`](crate::param_name)'s simple form), or any other
    /// [`ToBytes`] type (including a composite [`crate::HookData`] struct)
    /// for a structured name.
    type Name: ToBytes;

    /// The one name value identifying this parameter.
    const NAME: Self::Name;

    /// Returns this parameter's name as wire bytes, using `buf` as scratch
    /// space if encoding needs it.
    ///
    /// The default implementation always needs `buf`: it runs
    /// `Self::NAME.write(buf)` (the only option for an arbitrary
    /// [`ToBytes`] type) and returns the written prefix. `buf` must be at
    /// least [`PARAM_NAME_MAX_LEN`] bytes.
    ///
    /// [`param_name!`](crate::param_name)'s simple form (a plain byte
    /// array name) overrides this to return `&Self::NAME` directly,
    /// ignoring `buf` — see this trait's "Zero-cost" section.
    ///
    /// A compile-time check (monomorphized per `Self`) rejects a
    /// [`Name`](Self::Name) whose [`ToBytes::MAX_LEN`] falls outside
    /// `1..=`[`PARAM_NAME_MAX_LEN`] — the Hook API's own bound on a
    /// parameter name's length.
    #[inline(always)]
    fn name_bytes(buf: &mut [u8; PARAM_NAME_MAX_LEN]) -> &[u8] {
        const {
            assert!(
                <Self::Name as ToBytes>::MAX_LEN >= 1,
                "hooks_lib: ParamName::Name::MAX_LEN must be at least 1 byte \
                 (the Hook API's parameter-name lower bound)"
            );
            assert!(
                <Self::Name as ToBytes>::MAX_LEN <= PARAM_NAME_MAX_LEN,
                "hooks_lib: ParamName::Name::MAX_LEN exceeds the Hook API's \
                 32-byte parameter-name upper bound"
            );
        }
        let _ = Self::NAME.write(buf);
        buf.get(..<Self::Name as ToBytes>::MAX_LEN).unwrap_or(&[])
    }
}

/// Implements [`ParamName`] for `$Ty`, naming it — the one-line way to opt
/// a type into [`crate::api::hook_ctx::hook_param_kv`]/
/// [`crate::api::otxn::otxn_param_kv`]. Two forms:
///
/// - `param_name!($Ty, $name)` — `$name` a byte-string literal (or any
///   `&'static [u8; N]` expression); `Self::Name` is inferred as `[u8; N]`,
///   and [`ParamName::name_bytes`] is overridden to cost nothing (see
///   [`ParamName`]'s "Zero-cost" section). Covers the common case, a plain
///   short tag like `b"CFG"`.
/// - `param_name!($Ty, $Name, $value)` — `$Name` any [`ToBytes`] type
///   (commonly another [`crate::HookData`] struct) and `$value` the one
///   instance of it naming this parameter — for a **composite** parameter
///   name, the same idea as [`crate::state_key_value!`]'s composite state
///   key, just encoded at its own natural length instead of zero-padded.
///   Uses [`ParamName::name_bytes`]'s default body (a small, genuine
///   runtime encode — unavoidable for an arbitrary type).
///
/// ```
/// use hooks_lib::prelude::*;
/// use hooks_lib::{param_name, HookData};
///
/// #[derive(HookData)]
/// struct Config {
///     min_amount: u64,
/// }
///
/// param_name!(Config, b"CFG");
///
/// let cfg: Result<Config> = hook_param_kv();
/// assert_eq!(cfg.err(), Some(HookError::NotImplemented));
/// ```
///
/// A composite (struct-shaped) parameter name:
///
/// ```
/// use hooks_lib::prelude::*;
/// use hooks_lib::{param_name, HookData};
///
/// #[derive(HookData, Clone, Copy)]
/// struct SeatParamName {
///     topic: u8,
///     seat: u8,
/// }
///
/// #[derive(HookData)]
/// struct Vote {
///     value: u8,
/// }
///
/// param_name!(Vote, SeatParamName, SeatParamName { topic: b'S', seat: 0 });
///
/// let vote: Result<Vote> = otxn_param_kv();
/// assert_eq!(vote.err(), Some(HookError::NotImplemented));
/// ```
#[macro_export]
macro_rules! param_name {
    ($Ty:ty, $name:expr) => {
        impl $crate::convert::ParamName for $Ty {
            type Name = [u8; { $name.len() }];
            const NAME: Self::Name = *$name;

            #[inline(always)]
            fn name_bytes(_buf: &mut [u8; $crate::convert::PARAM_NAME_MAX_LEN]) -> &[u8] {
                &Self::NAME
            }
        }
    };
    ($Ty:ty, $Name:ty, $value:expr) => {
        impl $crate::convert::ParamName for $Ty {
            type Name = $Name;
            const NAME: Self::Name = $value;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_round_trips() {
        let mut buf = [0u8; 1];
        assert_eq!(0xABu8.write(&mut buf), 1);
        assert_eq!(buf, [0xAB]);
        assert_eq!(u8::read(&buf), Ok(0xABu8));
    }

    #[test]
    fn u8_write_into_empty_buffer_writes_nothing() {
        let mut buf: [u8; 0] = [];
        assert_eq!(0xABu8.write(&mut buf), 0);
    }

    #[test]
    fn u8_read_from_empty_buffer_fails() {
        assert_eq!(u8::read(&[]), Err(HookError::TooSmall));
    }

    #[test]
    fn u16_round_trips() {
        let mut buf = [0u8; 2];
        assert_eq!(0x0102u16.write(&mut buf), 2);
        assert_eq!(buf, 0x0102u16.to_le_bytes());
        assert_eq!(u16::read(&buf), Ok(0x0102u16));
    }

    #[test]
    fn u16_write_into_short_buffer_writes_nothing() {
        let mut buf = [0xFFu8; 1];
        assert_eq!(0x0102u16.write(&mut buf), 0);
        assert_eq!(buf, [0xFF]);
    }

    #[test]
    fn u16_read_from_short_buffer_fails() {
        assert_eq!(u16::read(&[0u8; 1]), Err(HookError::TooSmall));
    }

    #[test]
    fn u32_round_trips() {
        let mut buf = [0u8; 4];
        assert_eq!(42u32.write(&mut buf), 4);
        assert_eq!(buf, 42u32.to_le_bytes());
        assert_eq!(u32::read(&buf), Ok(42u32));
    }

    #[test]
    fn u32_write_into_short_buffer_writes_nothing() {
        let mut buf = [0xFFu8; 3];
        assert_eq!(42u32.write(&mut buf), 0);
        assert_eq!(buf, [0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn u32_read_from_short_buffer_fails() {
        assert_eq!(u32::read(&[0u8; 3]), Err(HookError::TooSmall));
    }

    #[test]
    fn u64_round_trips() {
        let mut buf = [0u8; 8];
        assert_eq!(0x0102_0304_0506_0708u64.write(&mut buf), 8);
        assert_eq!(u64::read(&buf), Ok(0x0102_0304_0506_0708u64));
    }

    #[test]
    fn i64_round_trips() {
        let mut buf = [0u8; 8];
        assert_eq!((-1i64).write(&mut buf), 8);
        assert_eq!(i64::read(&buf), Ok(-1i64));
    }

    #[test]
    fn fixed_array_round_trips() {
        let value = [1u8, 2, 3, 4, 5];
        let mut buf = [0u8; 5];
        assert_eq!(value.write(&mut buf), 5);
        assert_eq!(buf, value);
        assert_eq!(<[u8; 5]>::read(&buf), Ok(value));
    }

    #[test]
    fn xfl_round_trips_bit_pattern() {
        use crate::xfl::XFL;

        let value = XFL::from_raw_bits(0x1234_5678_9ABC_DEF0);
        let mut buf = [0u8; 8];
        assert_eq!(value.write(&mut buf), 8);
        assert_eq!(
            crate::xfl::XFL::read(&buf).map(XFL::raw_bits),
            Ok(0x1234_5678_9ABC_DEF0i64)
        );
    }

    #[test]
    fn fixed_read_array_succeeds_on_exact_write() {
        let result: Result<[u8; 4]> = <[u8; 4]>::read_exact(|buf| {
            buf.copy_from_slice(&[1, 2, 3, 4]);
            Ok(4)
        });
        assert_eq!(result, Ok([1, 2, 3, 4]));
    }

    #[test]
    fn fixed_read_array_rejects_short_write() {
        let result: Result<[u8; 4]> = <[u8; 4]>::read_exact(|_buf| Ok(3));
        assert_eq!(result, Err(HookError::TooSmall));
    }

    #[test]
    fn fixed_read_array_propagates_read_error() {
        let result: Result<[u8; 4]> = <[u8; 4]>::read_exact(|_buf| Err(HookError::InternalError));
        assert_eq!(result, Err(HookError::InternalError));
    }

    #[test]
    fn fixed_read_passes_a_buffer_of_exactly_n_bytes() {
        let _: Result<[u8; 7]> = <[u8; 7]>::read_exact(|buf| {
            assert_eq!(buf.len(), 7);
            Ok(buf.len())
        });
    }
}
