//! Zero-copy STObject (STO) reader: the guest-side, opt-in inverse of
//! [`crate::txn::codec`]'s field-header encoding, plus a small typed
//! [`PaymentView`] built on top of it.
//!
//! # Why this is opt-in, and what it costs
//!
//! Parsing an arbitrary serialized transaction (the whole-`otxn` blob from
//! [`crate::api::otxn::otxn_slot`] + [`crate::api::slot::slot`], or any
//! other STO byte buffer such as a [`crate::api::sto::sto_subfield`]
//! result) is genuinely guest-side code: every byte of [`StoReader`]'s
//! field-header/VL-length decoding and [`PaymentView`]'s typed accessors
//! is compiled into *your* hook's wasm, and — because `hooks-build`'s
//! flatten pass (`docs/DESIGN.md` §6.2b) inlines every call site — its
//! size and worst-case-instruction cost lands wherever it's used, not once
//! globally. Compare that with [`crate::api::otxn::otxn_field`] or
//! [`crate::api::sto::sto_subfield`]: those are single host calls, whose
//! field-lookup work is paid for by the *host*, not by wasm bytes/
//! instructions in your binary.
//!
//! **Rule of thumb**: reading a handful of *known* fields off a
//! transaction (`Amount`, `Destination`, a couple of tags) is usually
//! cheaper via [`crate::api::otxn::otxn_field`] directly, one host call per
//! field. Reach for this module when you genuinely need to *walk* an
//! object generically — an unknown number of fields, or a nested array
//! (`Memos`, `Signers`) — the sort of thing with no single-field host call
//! to ask for.
//!
//! If a hook never calls into this module, none of its code survives:
//! nothing else in `hooks-lib` references it, so `hooks-build`'s
//! reachability GC (`docs/DESIGN.md` §6.2 step 3) drops every function
//! here entirely — the unused-module cost is zero, not "small".
//!
//! # Scope (Phase 1)
//!
//! Decodes `UInt16`/`UInt32`/`UInt64`, `Hash128`/`Hash160`/`Hash256`,
//! `AccountID`, `Amount` (discriminating native XRP/XAH drops from IOU
//! amounts by the wire's top-bit convention — see [`AmountValue`]), and
//! `Blob`/VL fields. Nested `STObject`/`STArray` fields are **skip-only**:
//! [`StoReader::next`] returns their raw content bytes
//! ([`FieldValue::Nested`]) without parsing inside them — construct a
//! fresh [`StoReader::new`] over that slice yourself if you need to
//! recurse. Every other field type (`PathSet`, `Vector256`, `Number`, the
//! fixed `UInt96`/`UInt192`/`UInt384`/`UInt512` families, `Issue`, ...) is
//! reported as [`HookError::ParseError`] rather than guessed at — an
//! unrecognized-length guess would silently desynchronize the rest of the
//! scan.
//!
//! # Why no recursion
//!
//! `docs/DESIGN.md` §2 C5 bans recursion outright (the static
//! instruction-count analysis needs an acyclic call graph, and the
//! flatten pass — §6.2b — inlines by walking the call graph bottom-up,
//! which does not terminate for a self-referencing function). Skipping an
//! arbitrarily-nested `STObject`/`STArray` therefore cannot be written as
//! "call yourself on the inner object" — the private `scan_container`
//! helper instead tracks nesting with a plain `depth: u32` counter in a
//! single flat loop: entering a nested container increments `depth`, its
//! end marker decrements it, and the scan stops when a depth-`0` end
//! marker is found. No stack, no self-call.
//!
//! # Guarding
//!
//! [`StoReader::next`] is a plain, unguarded cursor-advance method — a
//! hand-written `loop { ... }` calling it directly **must** carry its own
//! [`crate::guard!`] per `docs/DESIGN.md` §2 C2 (see [`StoReader`]'s doc
//! example). [`StoReader::for_each_field`] is the guarded alternative: it
//! takes the iteration cap as an explicit `max_fields` argument and calls
//! `guard!` internally, so ordinary calling code doesn't need to.

use core::ops::ControlFlow;

use crate::error::{HookError, Result};
use crate::types::{
    ACC_ID_LEN, AccountId, HASH_LEN, Hash, IOU_AMOUNT_LEN, IouAmount, NATIVE_AMOUNT_LEN,
};

// --- STI type codes this module knows about (rippled/xahaud's
// `SerializedTypeID`; see `sfcodes.rs`'s `(type << 16) | field` packing,
// which these numbers are extracted from). Only the type codes this
// module's Phase 1 scope supports get a name; every other code falls
// through to `HookError::ParseError`. ---

const STI_UINT16: u32 = 1;
const STI_UINT32: u32 = 2;
const STI_UINT64: u32 = 3;
const STI_HASH128: u32 = 4;
const STI_HASH256: u32 = 5;
const STI_AMOUNT: u32 = 6;
const STI_VL: u32 = 7;
const STI_ACCOUNT: u32 = 8;
const STI_OBJECT: u32 = 14;
const STI_ARRAY: u32 = 15;
const STI_HASH160: u32 = 17;

/// Field code shared by both `sfObjectEndMarker` (paired with
/// [`STI_OBJECT`]) and `sfArrayEndMarker` (paired with [`STI_ARRAY`]) —
/// rippled/xahaud's nested-container terminator is field code `1` in both
/// type namespaces (wire bytes `0xE1`/`0xF1` respectively).
const END_MARKER_FIELD: u32 = 1;

/// Byte length of an [`FieldValue::Hash128`] value.
const HASH128_LEN: usize = 16;
/// Byte length of an [`FieldValue::Hash160`] value.
const HASH160_LEN: usize = 20;

/// Maximum field headers [`scan_container`] will decode while looking for
/// a nested container's end marker, across all nesting levels combined —
/// a defense against malformed/adversarial input looping indefinitely.
/// Legitimate protocol data needs nowhere near this many.
const MAX_CONTAINER_SCAN_FIELDS: u32 = 512;

/// Decodes one STObject field-id header at `buf[pos..]` — the inverse of
/// [`crate::txn::codec::field_header`]'s encoding. Returns
/// `(sfcode, header_len)`, `header_len` being `1`, `2`, or `3`.
///
/// Follows the canonical rippled/xahaud rule: the byte's high nibble is a
/// candidate type code and its low nibble a candidate field code; either
/// nibble reading back `0` means "uncommon — read the next byte instead"
/// (in type-then-field order). This correctly inverts every combination of
/// common/uncommon type and field, including the "uncommon type, common
/// field" case (e.g. `sfTakerPaysCurrency`, `STI_HASH160` field `1`).
///
/// # Errors
///
/// [`HookError::ParseError`] if `buf` is truncated, or an "uncommon"
/// follow-up byte reads back below `16` (which never happens for a
/// correctly encoded header — that value would have been encoded in the
/// first byte's nibble instead).
fn decode_field_header(buf: &[u8], pos: usize) -> Result<(u32, usize)> {
    let b0 = u32::from(buf.get(pos).copied().ok_or(HookError::ParseError)?);
    let mut ty = b0 >> 4;
    let mut field = b0 & 0xF;
    let mut len: usize = 1;
    if ty == 0 {
        let b = u32::from(
            buf.get(pos.wrapping_add(len))
                .copied()
                .ok_or(HookError::ParseError)?,
        );
        if b < 16 {
            return Err(HookError::ParseError);
        }
        ty = b;
        len = len.wrapping_add(1);
    }
    if field == 0 {
        let b = u32::from(
            buf.get(pos.wrapping_add(len))
                .copied()
                .ok_or(HookError::ParseError)?,
        );
        if b < 16 {
            return Err(HookError::ParseError);
        }
        field = b;
        len = len.wrapping_add(1);
    }
    Ok(((ty << 16) | field, len))
}

/// Decodes a rippled/xahaud VL (variable-length) length prefix at
/// `buf[pos..]` — the inverse of the length half of upstream's
/// `Serializer::addVL`. Returns `(payload_len, header_len)`; `header_len`
/// is `1`, `2`, or `3` depending on which of the three length ranges
/// `payload_len` falls in.
///
/// # Errors
///
/// [`HookError::ParseError`] if `buf` is truncated, or the leading byte is
/// `255` (reserved — not a valid VL length prefix).
fn decode_vl_length(buf: &[u8], pos: usize) -> Result<(usize, usize)> {
    let b0 = usize::from(buf.get(pos).copied().ok_or(HookError::ParseError)?);
    if b0 <= 192 {
        Ok((b0, 1))
    } else if b0 <= 240 {
        let b1 = usize::from(
            buf.get(pos.wrapping_add(1))
                .copied()
                .ok_or(HookError::ParseError)?,
        );
        let len = 193usize
            .wrapping_add(b0.wrapping_sub(193).wrapping_mul(256))
            .wrapping_add(b1);
        Ok((len, 2))
    } else if b0 <= 254 {
        let b1 = usize::from(
            buf.get(pos.wrapping_add(1))
                .copied()
                .ok_or(HookError::ParseError)?,
        );
        let b2 = usize::from(
            buf.get(pos.wrapping_add(2))
                .copied()
                .ok_or(HookError::ParseError)?,
        );
        let len = 12481usize
            .wrapping_add(b0.wrapping_sub(241).wrapping_mul(65536))
            .wrapping_add(b1.wrapping_mul(256))
            .wrapping_add(b2);
        Ok((len, 3))
    } else {
        Err(HookError::ParseError)
    }
}

/// Reads `N` bytes at `buf[start..start+N]` into an owned array.
///
/// # Errors
///
/// [`HookError::ParseError`] if `buf` is too short.
fn read_array<const N: usize>(buf: &[u8], start: usize) -> Result<[u8; N]> {
    let end = start.wrapping_add(N);
    let raw = buf.get(start..end).ok_or(HookError::ParseError)?;
    raw.try_into()
        .map_err(|_: core::array::TryFromSliceError| HookError::ParseError)
}

/// Byte length of the *value* portion of a field of STI type `ty`,
/// starting at `value_start` — used by both [`StoReader::next`] and
/// [`scan_container`] to skip a field without decoding it. Never called
/// for `ty` in {[`STI_OBJECT`], [`STI_ARRAY`]}: those have no fixed value
/// length (callers special-case them instead).
///
/// # Errors
///
/// [`HookError::ParseError`] for a truncated `Amount`/VL length prefix, or
/// any `ty` outside this module's Phase 1 scope.
fn value_len(buf: &[u8], ty: u32, value_start: usize) -> Result<usize> {
    match ty {
        STI_UINT16 => Ok(2),
        STI_UINT32 => Ok(4),
        STI_UINT64 => Ok(8),
        STI_HASH128 => Ok(HASH128_LEN),
        STI_HASH256 => Ok(HASH_LEN),
        STI_HASH160 => Ok(HASH160_LEN),
        STI_AMOUNT => {
            let b0 = buf.get(value_start).copied().ok_or(HookError::ParseError)?;
            Ok(if b0 & 0x80 == 0 {
                NATIVE_AMOUNT_LEN
            } else {
                IOU_AMOUNT_LEN
            })
        }
        STI_VL | STI_ACCOUNT => {
            let (payload_len, hdr_len) = decode_vl_length(buf, value_start)?;
            Ok(hdr_len.wrapping_add(payload_len))
        }
        _ => Err(HookError::ParseError),
    }
}

/// Scans forward from `start` (the first byte *after* a nested
/// `STObject`/`STArray` field's own header) to find that container's
/// matching end marker, tracking further nesting with a flat `depth`
/// counter instead of recursion (see the module doc comment's "Why no
/// recursion" section). Returns `(content_len, marker_len)`:
/// `buf[start..start+content_len]` is the container's raw inner content
/// (its own nested fields, not including the end marker itself), and
/// `marker_len` is the end marker's own header length (always `1`).
///
/// # Errors
///
/// [`HookError::ParseError`] on truncated/malformed input, an unsupported
/// nested field type, or exceeding [`MAX_CONTAINER_SCAN_FIELDS`] headers
/// without finding the matching end marker.
#[allow(clippy::arithmetic_side_effects, clippy::macro_metavars_in_unsafe)] // both fire on `guard!`'s own expansion (`crate::macros`), not on code written here
fn scan_container(buf: &[u8], start: usize) -> Result<(usize, usize)> {
    let mut pos = start;
    let mut depth: u32 = 0;
    let mut scanned: u32 = 0;
    loop {
        crate::guard!(MAX_CONTAINER_SCAN_FIELDS);
        if scanned >= MAX_CONTAINER_SCAN_FIELDS {
            return Err(HookError::ParseError);
        }
        scanned = scanned.wrapping_add(1);

        let (sfcode, hdr_len) = decode_field_header(buf, pos)?;
        let ty = sfcode >> 16;
        let field = sfcode & 0xFFFF;
        let is_end_marker = (ty == STI_OBJECT || ty == STI_ARRAY) && field == END_MARKER_FIELD;

        if is_end_marker {
            if depth == 0 {
                let content_len = pos.wrapping_sub(start);
                return Ok((content_len, hdr_len));
            }
            depth = depth.wrapping_sub(1);
            pos = pos.wrapping_add(hdr_len);
            continue;
        }

        if ty == STI_OBJECT || ty == STI_ARRAY {
            depth = depth.wrapping_add(1);
            pos = pos.wrapping_add(hdr_len);
            continue;
        }

        let value_start = pos.wrapping_add(hdr_len);
        let vlen = value_len(buf, ty, value_start)?;
        pos = value_start.wrapping_add(vlen);
    }
}

/// One decoded STObject field: its `sfXxx` code and typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<'a> {
    /// The field's `sfcode` (`(type << 16) | field`, the same encoding
    /// [`crate::txn::codec::field_header`] takes).
    pub sfcode: u32,
    /// The decoded value; see [`FieldValue`].
    pub value: FieldValue<'a>,
}

impl Field<'_> {
    /// The field's STI type code (`sfcode >> 16`).
    #[must_use]
    pub fn field_type(&self) -> u32 {
        self.sfcode >> 16
    }

    /// The field's field code (`sfcode & 0xFFFF`).
    #[must_use]
    pub fn field_code(&self) -> u32 {
        self.sfcode & 0xFFFF
    }
}

/// A decoded field value, per [`StoReader`]'s Phase 1 type scope (see the
/// module doc comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValue<'a> {
    /// `STI_UINT16`.
    UInt16(u16),
    /// `STI_UINT32`.
    UInt32(u32),
    /// `STI_UINT64`.
    UInt64(u64),
    /// `STI_HASH128` (16 bytes).
    Hash128([u8; HASH128_LEN]),
    /// `STI_HASH160` (20 bytes).
    Hash160([u8; HASH160_LEN]),
    /// `STI_HASH256` (32 bytes).
    Hash256(Hash),
    /// `STI_ACCOUNT` (VL-encoded, always exactly 20 bytes).
    AccountId(AccountId),
    /// `STI_AMOUNT`; see [`AmountValue`].
    Amount(AmountValue),
    /// `STI_VL`: a variable-length blob, borrowed from the source buffer.
    Blob(&'a [u8]),
    /// `STI_OBJECT`/`STI_ARRAY`: **not parsed** (Phase 1 scope) — `bytes`
    /// is the container's raw inner content (its fields, without the end
    /// marker); construct a fresh [`StoReader::new`] over it to recurse.
    Nested {
        /// `true` for `STI_ARRAY`, `false` for `STI_OBJECT`.
        is_array: bool,
        /// The container's raw inner content.
        bytes: &'a [u8],
    },
}

/// A decoded `STI_AMOUNT` value, discriminated by the wire's top-bit
/// convention (see [`crate::txn::codec::encode_native_amount_const`]'s doc
/// comment for the write-side mirror of this rule): the value's leading
/// bit clear ⇒ native (8-byte) drops, set ⇒ a 48-byte IOU amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountValue {
    /// Native XRP/XAH drops (the two leading control bits are already
    /// masked off — this is the plain magnitude).
    Native(u64),
    /// A 48-byte IOU amount: 8-byte mantissa/exponent/sign, 20-byte
    /// currency code, 20-byte issuer `AccountID` — raw wire bytes,
    /// undecomposed (Phase 1 scope; see the module doc comment).
    Iou(IouAmount),
}

/// A zero-copy, forward-only cursor over a serialized STObject's top-level
/// fields (no nested-container recursion; see the module doc comment).
///
/// # Guarding
///
/// A hand-written loop calling [`Self::next`] must carry its own
/// [`crate::guard!`] (`docs/DESIGN.md` §2 C2); [`Self::for_each_field`] is
/// the guarded, higher-level alternative.
///
/// # Examples
///
/// ```
/// use hooks_lib::guard;
/// use hooks_lib::sto_read::{FieldValue, StoReader};
///
/// // A single STI_UINT32 `Sequence` field (sfcode (2<<16)+4), value 7:
/// // header 0x24, then 7 big-endian.
/// let bytes = [0x24, 0, 0, 0, 7];
/// let mut reader = StoReader::new(&bytes);
/// let mut count = 0;
/// loop {
///     guard!(4);
///     match reader.next() {
///         None => break,
///         Some(Ok(field)) => {
///             assert_eq!(field.sfcode, (2 << 16) + 4);
///             assert_eq!(field.value, FieldValue::UInt32(7));
///             count += 1;
///         }
///         Some(Err(_)) => unreachable!("well-formed input"),
///     }
/// }
/// assert_eq!(count, 1);
/// ```
pub struct StoReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> StoReader<'a> {
    /// Wraps `buf` for field-by-field scanning, starting at offset `0`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Decodes and returns the next field, or `None` once the buffer is
    /// exhausted. See the type's doc comment for the guarding requirement.
    ///
    /// A malformed field does **not** advance the cursor — calling `next`
    /// again after `Some(Err(_))` reproduces the same error rather than
    /// silently skipping the bad byte, so callers should treat any `Err`
    /// as "stop".
    #[allow(clippy::should_implement_trait)] // deliberately not `Iterator`: guarding (see the type's doc comment) is the caller's responsibility, which `for field in reader` would obscure
    pub fn next(&mut self) -> Option<Result<Field<'a>>> {
        if self.pos >= self.buf.len() {
            return None;
        }
        Some(self.decode_one())
    }

    /// Guarded, higher-level scan: calls `f` once per decoded field, up to
    /// `max_fields` fields, stopping early if `f` returns
    /// [`ControlFlow::Break`]. Calls [`crate::guard!`] internally, so the
    /// caller does not need to wrap its own loop.
    ///
    /// # Errors
    ///
    /// Propagates the first malformed-field error from [`Self::next`], if
    /// any (`f` may already have run for the fields decoded before it).
    #[allow(clippy::arithmetic_side_effects, clippy::macro_metavars_in_unsafe)] // both fire on `guard!`'s own expansion (`crate::macros`), not on code written here
    pub fn for_each_field<F>(&mut self, max_fields: u32, mut f: F) -> Result<()>
    where
        F: FnMut(Field<'a>) -> ControlFlow<()>,
    {
        let mut i: u32 = 0;
        loop {
            crate::guard!(max_fields);
            if i >= max_fields {
                break;
            }
            match self.next() {
                None => break,
                Some(Err(e)) => return Err(e),
                Some(Ok(field)) => {
                    if let ControlFlow::Break(()) = f(field) {
                        break;
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        Ok(())
    }

    fn decode_one(&mut self) -> Result<Field<'a>> {
        let buf = self.buf;
        let (sfcode, hdr_len) = decode_field_header(buf, self.pos)?;
        let ty = sfcode >> 16;
        let value_start = self.pos.wrapping_add(hdr_len);
        let (value, new_pos): (FieldValue<'a>, usize) = match ty {
            STI_UINT16 => (
                FieldValue::UInt16(u16::from_be_bytes(read_array(buf, value_start)?)),
                value_start.wrapping_add(2),
            ),
            STI_UINT32 => (
                FieldValue::UInt32(u32::from_be_bytes(read_array(buf, value_start)?)),
                value_start.wrapping_add(4),
            ),
            STI_UINT64 => (
                FieldValue::UInt64(u64::from_be_bytes(read_array(buf, value_start)?)),
                value_start.wrapping_add(8),
            ),
            STI_HASH128 => (
                FieldValue::Hash128(read_array(buf, value_start)?),
                value_start.wrapping_add(HASH128_LEN),
            ),
            STI_HASH160 => (
                FieldValue::Hash160(read_array(buf, value_start)?),
                value_start.wrapping_add(HASH160_LEN),
            ),
            STI_HASH256 => (
                FieldValue::Hash256(Hash(read_array(buf, value_start)?)),
                value_start.wrapping_add(HASH_LEN),
            ),
            STI_AMOUNT => {
                let b0 = buf.get(value_start).copied().ok_or(HookError::ParseError)?;
                if b0 & 0x80 == 0 {
                    let raw: [u8; NATIVE_AMOUNT_LEN] = read_array(buf, value_start)?;
                    // Top 2 bits are native-amount control flags (see
                    // `codec::encode_native_amount_const`), not part of
                    // the magnitude.
                    let v = u64::from_be_bytes(raw) & 0x3FFF_FFFF_FFFF_FFFF;
                    (
                        FieldValue::Amount(AmountValue::Native(v)),
                        value_start.wrapping_add(NATIVE_AMOUNT_LEN),
                    )
                } else {
                    let raw = IouAmount(read_array(buf, value_start)?);
                    (
                        FieldValue::Amount(AmountValue::Iou(raw)),
                        value_start.wrapping_add(IOU_AMOUNT_LEN),
                    )
                }
            }
            STI_VL => {
                let (payload_len, vl_hdr) = decode_vl_length(buf, value_start)?;
                let payload_start = value_start.wrapping_add(vl_hdr);
                let payload_end = payload_start.wrapping_add(payload_len);
                let raw = buf
                    .get(payload_start..payload_end)
                    .ok_or(HookError::ParseError)?;
                (FieldValue::Blob(raw), payload_end)
            }
            STI_ACCOUNT => {
                let (payload_len, vl_hdr) = decode_vl_length(buf, value_start)?;
                if payload_len != ACC_ID_LEN {
                    return Err(HookError::ParseError);
                }
                let payload_start = value_start.wrapping_add(vl_hdr);
                let id = AccountId(read_array(buf, payload_start)?);
                (
                    FieldValue::AccountId(id),
                    payload_start.wrapping_add(ACC_ID_LEN),
                )
            }
            STI_OBJECT | STI_ARRAY => {
                let (content_len, marker_len) = scan_container(buf, value_start)?;
                let end = value_start.wrapping_add(content_len);
                let bytes = buf.get(value_start..end).ok_or(HookError::ParseError)?;
                (
                    FieldValue::Nested {
                        is_array: ty == STI_ARRAY,
                        bytes,
                    },
                    end.wrapping_add(marker_len),
                )
            }
            _ => return Err(HookError::ParseError),
        };
        self.pos = new_pos;
        Ok(Field { sfcode, value })
    }
}

/// Bound passed to [`StoReader::for_each_field`] by every [`PaymentView`]
/// accessor: a `Payment` (even a field-heavy one — flags, tags, a
/// destination, an un-expanded `Memos`/paths array, ...) has nowhere near
/// this many top-level fields; this exists to give the underlying scan a
/// concrete, honest guard bound rather than an unbounded one.
const PAYMENT_VIEW_MAX_FIELDS: u32 = 32;

/// A typed, read-only view over a serialized `Payment`-shaped transaction
/// buffer — e.g. from [`crate::api::otxn::otxn_slot`] +
/// [`crate::api::slot::slot`], or from [`crate::api::otxn::otxn_field`]
/// with a field ID that names the whole transaction. Every accessor does
/// its own bounded [`StoReader`] scan; see the module doc comment for the
/// guest-side cost this implies relative to
/// [`crate::api::otxn::otxn_field`].
pub struct PaymentView<'a> {
    buf: &'a [u8],
}

impl<'a> PaymentView<'a> {
    /// Wraps `buf`. No validation is performed until an accessor is
    /// called.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// Finds the first top-level field with `sfcode`, if any, scanning at
    /// most [`PAYMENT_VIEW_MAX_FIELDS`] fields.
    fn find(&self, sfcode: u32) -> Result<Option<FieldValue<'a>>> {
        let mut reader = StoReader::new(self.buf);
        let mut found: Option<FieldValue<'a>> = None;
        reader.for_each_field(PAYMENT_VIEW_MAX_FIELDS, |field| {
            if field.sfcode == sfcode {
                found = Some(field.value);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        Ok(found)
    }

    /// The `Amount` field.
    ///
    /// # Errors
    ///
    /// [`HookError::DoesntExist`] if absent; [`HookError::ParseError`] if
    /// present but not wire-shaped as an `STI_AMOUNT`, or on any
    /// underlying scan error (truncated/malformed buffer).
    pub fn amount(&self) -> Result<AmountValue> {
        match self.find(hooks_core::sfcodes::sfAmount)? {
            Some(FieldValue::Amount(a)) => Ok(a),
            Some(_) => Err(HookError::ParseError),
            None => Err(HookError::DoesntExist),
        }
    }

    /// The `Destination` field.
    ///
    /// # Errors
    ///
    /// See [`Self::amount`].
    pub fn destination(&self) -> Result<AccountId> {
        match self.find(hooks_core::sfcodes::sfDestination)? {
            Some(FieldValue::AccountId(id)) => Ok(id),
            Some(_) => Err(HookError::ParseError),
            None => Err(HookError::DoesntExist),
        }
    }

    /// The `Account` (sender) field.
    ///
    /// # Errors
    ///
    /// See [`Self::amount`].
    pub fn account(&self) -> Result<AccountId> {
        match self.find(hooks_core::sfcodes::sfAccount)? {
            Some(FieldValue::AccountId(id)) => Ok(id),
            Some(_) => Err(HookError::ParseError),
            None => Err(HookError::DoesntExist),
        }
    }

    /// The `Flags` field.
    ///
    /// # Errors
    ///
    /// See [`Self::amount`].
    pub fn flags(&self) -> Result<u32> {
        match self.find(hooks_core::sfcodes::sfFlags)? {
            Some(FieldValue::UInt32(v)) => Ok(v),
            Some(_) => Err(HookError::ParseError),
            None => Err(HookError::DoesntExist),
        }
    }

    /// The `Sequence` field.
    ///
    /// # Errors
    ///
    /// See [`Self::amount`].
    pub fn sequence(&self) -> Result<u32> {
        match self.find(hooks_core::sfcodes::sfSequence)? {
            Some(FieldValue::UInt32(v)) => Ok(v),
            Some(_) => Err(HookError::ParseError),
            None => Err(HookError::DoesntExist),
        }
    }

    /// The optional `DestinationTag` field.
    ///
    /// # Errors
    ///
    /// [`HookError::ParseError`] if present but not an `STI_UINT32`, or on
    /// any underlying scan error. Absence is **not** an error: returns
    /// `Ok(None)`.
    pub fn destination_tag(&self) -> Result<Option<u32>> {
        match self.find(hooks_core::sfcodes::sfDestinationTag)? {
            Some(FieldValue::UInt32(v)) => Ok(Some(v)),
            Some(_) => Err(HookError::ParseError),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests are exempt from the panic-freedom lints (see docs/DESIGN.md
    // §8); expect/indexing on known-good values is idiomatic here.
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::types::EMIT_DETAILS_MAX_LEN;
    use hooks_core::sfcodes::{
        sfAccount, sfDestination, sfDestinationTag, sfFee, sfFirstLedgerSequence,
        sfLastLedgerSequence, sfSequence, sfSigningPubKey,
    };

    #[test]
    fn uint32_field_decodes_and_exhausts_buffer() {
        let bytes = [0x24, 0, 0, 0, 7]; // Sequence (2,4) = 7
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.sfcode, (2 << 16) + 4);
        assert_eq!(field.value, FieldValue::UInt32(7));
        assert_eq!(field.field_type(), 2);
        assert_eq!(field.field_code(), 4);
        assert!(reader.next().is_none());
    }

    #[test]
    fn hash256_field_decodes() {
        // sfLedgerHash (5,1) -> single-byte header 0x51.
        let mut bytes = [0u8; 33];
        bytes[0] = 0x51;
        bytes[1..].copy_from_slice(&[0xAA; 32]);
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.value, FieldValue::Hash256(Hash([0xAA; 32])));
        assert!(reader.next().is_none());
    }

    #[test]
    fn hash128_field_decodes() {
        // sfEmailHash (4,1) -> single-byte header 0x41.
        let mut bytes = [0u8; 17];
        bytes[0] = 0x41;
        bytes[1..].copy_from_slice(&[0xBB; 16]);
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.value, FieldValue::Hash128([0xBB; 16]));
    }

    #[test]
    fn hash160_field_with_uncommon_type_common_field_header_decodes() {
        // sfTakerPaysCurrency: STI_HASH160 (17, uncommon) / field 1
        // (common) -> byte0 = 0x01 (type nibble 0 = "uncommon, read next
        // byte"; field nibble = 1), byte1 = 0x11 (17, the real type).
        let mut bytes = [0u8; 22];
        bytes[0] = 0x01;
        bytes[1] = 0x11;
        bytes[2..].copy_from_slice(&[0xCC; 20]);
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.sfcode, (17 << 16) + 1);
        assert_eq!(field.value, FieldValue::Hash160([0xCC; 20]));
    }

    #[test]
    fn native_amount_field_masks_control_bits() {
        // sfAmount (6,1) -> single-byte header 0x61, native 1 drop.
        let bytes = [0x61, 0x40, 0, 0, 0, 0, 0, 0, 1];
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.value, FieldValue::Amount(AmountValue::Native(1)));
    }

    #[test]
    fn iou_amount_field_decodes_as_48_raw_bytes() {
        let mut bytes = [0u8; 49];
        bytes[0] = 0x61; // sfAmount header
        bytes[1] = 0x80; // non-native marker bit set
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        let mut expected = [0u8; 48];
        expected[0] = 0x80;
        assert_eq!(
            field.value,
            FieldValue::Amount(AmountValue::Iou(IouAmount(expected)))
        );
    }

    #[test]
    fn blob_field_decodes() {
        // sfSigningPubKey (7,3) -> single-byte header 0x73, VL len 3.
        let bytes = [0x73, 3, 0xAA, 0xBB, 0xCC];
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.value, FieldValue::Blob(&[0xAA, 0xBB, 0xCC]));
    }

    #[test]
    fn account_id_field_decodes() {
        // sfAccount (8,1) -> single-byte header 0x81, VL len 20.
        let mut bytes = [0u8; 22];
        bytes[0] = 0x81;
        bytes[1] = 20;
        bytes[2..].copy_from_slice(&[0xEE; 20]);
        let mut reader = StoReader::new(&bytes);
        let field = reader.next().expect("one field").expect("well-formed");
        assert_eq!(field.value, FieldValue::AccountId(AccountId([0xEE; 20])));
    }

    #[test]
    fn account_id_field_rejects_wrong_vl_length() {
        let bytes = [0x81, 5, 0, 0, 0, 0, 0];
        let mut reader = StoReader::new(&bytes);
        assert_eq!(reader.next(), Some(Err(HookError::ParseError)));
    }

    #[test]
    fn unsupported_type_errors_instead_of_guessing() {
        // STI_NUMBER (9), field 1 -> single-byte header 0x91.
        let bytes = [0x91];
        let mut reader = StoReader::new(&bytes);
        assert_eq!(reader.next(), Some(Err(HookError::ParseError)));
    }

    #[test]
    fn truncated_fixed_width_field_errors_not_panics() {
        // sfSequence header claims a 4-byte UInt32, only 2 bytes follow.
        let bytes = [0x24, 0, 0];
        let mut reader = StoReader::new(&bytes);
        assert_eq!(reader.next(), Some(Err(HookError::ParseError)));
    }

    #[test]
    fn oversized_vl_length_errors_not_panics() {
        // VL byte 0xFE (254) requests the 3-byte form; declares a huge
        // length far beyond the (nonexistent) remaining buffer.
        let bytes = [0x73, 0xFE, 0xFF, 0xFF];
        let mut reader = StoReader::new(&bytes);
        assert_eq!(reader.next(), Some(Err(HookError::ParseError)));
    }

    #[test]
    fn reserved_vl_length_byte_errors() {
        let bytes = [0x73, 0xFF];
        let mut reader = StoReader::new(&bytes);
        assert_eq!(reader.next(), Some(Err(HookError::ParseError)));
    }

    #[test]
    fn skips_nested_stobject_and_continues_to_sibling_field() {
        // sfSequence(2,4)=7, then sfMemo(14,10) containing one inner
        // sfTransactionType(1,2)=5 field and its STObject end marker
        // (0xE1), then sfDestinationTag(2,14)=42.
        #[rustfmt::skip]
        let bytes = [
            0x24, 0, 0, 0, 7,       // Sequence = 7
            0xEA, 0x12, 0, 5, 0xE1, // Memo { TransactionType = 5 }
            0x2E, 0, 0, 0, 42,      // DestinationTag = 42
        ];
        let mut reader = StoReader::new(&bytes);

        let f1 = reader.next().expect("field 1").expect("well-formed");
        assert_eq!(f1.value, FieldValue::UInt32(7));

        let f2 = reader.next().expect("field 2").expect("well-formed");
        assert_eq!(f2.sfcode, (14 << 16) + 10);
        assert_eq!(
            f2.value,
            FieldValue::Nested {
                is_array: false,
                bytes: &[0x12, 0, 5]
            }
        );

        let f3 = reader.next().expect("field 3").expect("well-formed");
        assert_eq!(f3.value, FieldValue::UInt32(42));

        assert!(reader.next().is_none());
    }

    #[test]
    fn for_each_field_stops_early_on_control_flow_break() {
        let bytes = [
            0x24, 0, 0, 0, 1, // Sequence = 1
            0x2E, 0, 0, 0, 2, // DestinationTag = 2
        ];
        let mut reader = StoReader::new(&bytes);
        let mut seen = 0u32;
        reader
            .for_each_field(8, |_field| {
                seen += 1;
                ControlFlow::Break(())
            })
            .expect("no scan error");
        assert_eq!(seen, 1);
    }

    #[test]
    fn for_each_field_respects_max_fields_cap() {
        let bytes = [
            0x24, 0, 0, 0, 1, // Sequence = 1
            0x2E, 0, 0, 0, 2, // DestinationTag = 2
        ];
        let mut reader = StoReader::new(&bytes);
        let mut seen = 0u32;
        reader
            .for_each_field(1, |_field| {
                seen += 1;
                ControlFlow::Continue(())
            })
            .expect("no scan error");
        assert_eq!(seen, 1);
    }

    // --- Round-trip against `txn_template!`'s own encoder: the most
    // important test in this module (per the task brief) — proves the
    // decoder genuinely inverts the encoder, not just an independently
    // hand-rolled idea of the wire format. Deliberately declares no field
    // with STI type >= 16 and field < 16 (see
    // `hash160_field_with_uncommon_type_common_field_header_decodes`
    // above for that case in isolation): `txn_template!`'s
    // `codec::field_header` and this module's `decode_field_header` are
    // independently-derived inverses of each other for every combination
    // actually exercised by real transaction shapes like `Payment`.

    crate::txn_template! {
        /// Round-trip fixture with no `DestinationTag`, to exercise
        /// `PaymentView::destination_tag()`'s `Ok(None)` absent-field path.
        struct RoundTripPayment {
            transaction_type = ttPAYMENT,
            flags: u32_field(hooks_core::sfcodes::sfFlags) = 0,
            sequence: u32_field(sfSequence) = 0,
            first_ledger_sequence: u32_field(sfFirstLedgerSequence) = 0,
            last_ledger_sequence: u32_field(sfLastLedgerSequence) = 0,
            amount: native_amount(hooks_core::sfcodes::sfAmount) = 0,
            fee: native_amount(sfFee) = 0,
            signing_pub_key: empty_vl(sfSigningPubKey),
            account: account_id(sfAccount),
            destination: account_id(sfDestination),
            emit_details: emit_details,
        }
    }

    #[test]
    fn payment_view_round_trips_against_txn_template_encoder() {
        let mut tpl = RoundTripPayment::new();
        tpl.set_flags(0x8000_0000);
        tpl.set_sequence(42);
        tpl.set_amount(123_456).expect("in range");
        let dest = AccountId([0xAB; ACC_ID_LEN]);
        tpl.set_destination(&dest);
        let acct = AccountId([0xCD; ACC_ID_LEN]);
        tpl.set_account(&acct);

        // Exercise every other generated method too (dead-code hygiene,
        // same rationale as `txn.rs`'s `QualifiedPathAccount` test): none
        // of these affect the fields asserted below, and `prepare_for_emit`
        // fails on its very first host call (host stub), before writing
        // anything.
        tpl.set_first_ledger_sequence(0);
        tpl.set_last_ledger_sequence(0);
        tpl.set_fee(0).expect("0 drops is in range");
        let _ = tpl.emit_details_region();
        // `Prepared` (the `Ok` variant) doesn't implement `PartialEq` — only
        // the error path is ever compared here — so pull the `Err` out with
        // `expect_err` first rather than `assert_eq!`ing the whole `Result`
        // (see `txn.rs`'s `prepare_for_emit_propagates_host_stub_errors`).
        assert_eq!(
            tpl.prepare_for_emit()
                .expect_err("prepare_for_emit must fail on the host stub"),
            crate::error::HookError::NotImplemented
        );

        let fixed_len = RoundTripPayment::LEN - EMIT_DETAILS_MAX_LEN;
        let wire = &tpl.bytes()[..fixed_len];
        let view = PaymentView::new(wire);

        assert_eq!(view.flags().expect("present"), 0x8000_0000);
        assert_eq!(view.sequence().expect("present"), 42);
        assert_eq!(
            view.amount().expect("present"),
            AmountValue::Native(123_456)
        );
        assert_eq!(view.destination().expect("present"), dest);
        assert_eq!(view.account().expect("present"), acct);
        assert_eq!(view.destination_tag().expect("no scan error"), None);
    }

    crate::txn_template! {
        /// Round-trip fixture *with* `DestinationTag`, to exercise
        /// `PaymentView::destination_tag()`'s `Ok(Some(_))` present-field
        /// path, and (having no `Amount` field at all) the
        /// `Err(HookError::DoesntExist)` absent-*required*-field path.
        struct RoundTripPaymentWithTag {
            transaction_type = ttPAYMENT,
            sequence: u32_field(sfSequence) = 0,
            destination_tag: u32_field(sfDestinationTag) = 0,
            first_ledger_sequence: u32_field(sfFirstLedgerSequence) = 0,
            last_ledger_sequence: u32_field(sfLastLedgerSequence) = 0,
            fee: native_amount(sfFee) = 0,
            signing_pub_key: empty_vl(sfSigningPubKey),
            account: account_id(sfAccount),
            emit_details: emit_details,
        }
    }

    #[test]
    fn payment_view_destination_tag_present() {
        let mut tpl = RoundTripPaymentWithTag::new();
        tpl.set_destination_tag(999);

        // Dead-code hygiene, as in `payment_view_round_trips_against_txn_template_encoder`.
        tpl.set_sequence(0);
        tpl.set_first_ledger_sequence(0);
        tpl.set_last_ledger_sequence(0);
        tpl.set_fee(0).expect("0 drops is in range");
        tpl.set_account(&AccountId([0u8; ACC_ID_LEN]));
        let _ = tpl.emit_details_region();
        assert_eq!(
            tpl.prepare_for_emit()
                .expect_err("prepare_for_emit must fail on the host stub"),
            crate::error::HookError::NotImplemented
        );

        let fixed_len = RoundTripPaymentWithTag::LEN - EMIT_DETAILS_MAX_LEN;
        let view = PaymentView::new(&tpl.bytes()[..fixed_len]);
        assert_eq!(view.destination_tag().expect("no scan error"), Some(999));
    }

    #[test]
    fn payment_view_errors_when_required_field_absent() {
        let tpl = RoundTripPaymentWithTag::new();
        let fixed_len = RoundTripPaymentWithTag::LEN - EMIT_DETAILS_MAX_LEN;
        let view = PaymentView::new(&tpl.bytes()[..fixed_len]);
        assert_eq!(view.amount(), Err(HookError::DoesntExist));
    }
}
