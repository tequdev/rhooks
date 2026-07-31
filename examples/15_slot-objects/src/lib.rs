//! `slot-objects` — the live acceptance harness for the **typed slot layer**
//! (`hooks_lib::slot_obj`).
//!
//! Every check here exists because a host build *cannot* prove it. The
//! integration tests in `crates/hooks-lib/tests/slot_object.rs` cover typing,
//! inference and reachability; the host stubs there return `NotImplemented`
//! for every call, so nothing about real slot behavior can be observed. These
//! run against a real node and answer the questions the design had to assume:
//!
//! 1. **Account-root walk.** `SlotObject::from_keylet` on an account keylet,
//!    then typed reads of `sfBalance`/`sfSequence`/`sfAccount`.
//! 2. **The drops round-trip.** `as_xfl()` on a *native* amount yields XAH
//!    units, not drops — mantissa = drops, exponent −6, normalized. So
//!    `XFL::to_int(6, false)` (the host`s `float_int`) must recover the drop
//!    count exactly. This is
//!    the fact round 1 of the design review corrected by a factor of 10⁶.
//! 3. **Parent-clear then child-read.** `slot_path!` clears each intermediate
//!    as soon as its child exists, which is only sound if the host *copies*
//!    the parent's storage into the child slot. Verified here rather than
//!    assumed.
//! 4. **`take_*` recycling beyond the 255-slot budget.** A loop that derives
//!    a child per iteration and reads it with `take_value()` must survive
//!    more than 255 iterations; the same loop with a plain `value()` would
//!    exhaust the slots. Proves the recycling family actually frees.
//! 5. **Mid-hop failure leaks nothing.** A `slot_path!` walk over the
//!    sender's *SignerList* whose first two hops succeed — allocating two
//!    owned intermediates — and whose third fails, repeated past the budget.
//!    The ladder clears each intermediate unconditionally, so this must not
//!    exhaust slots. (An earlier version of this check walked a field absent
//!    from the account root: the *first*, borrowed-root hop failed, no owned
//!    intermediate was ever allocated, and the check was vacuous. The
//!    e2e installs a SignerList precisely so the hops before the failure are
//!    real.)
//! 6. **Repeated successful deep navigation.** The same SignerList walk
//!    taken to a real leaf, 300 times: the success path must recycle too.
//! 7. **Failure-path `take_*` cleanup.** `take_value()` on a slot whose read
//!    fails still clears, so a loop of *failing* reads must not exhaust the
//!    budget either — the half of the `take_*` contract the success loop
//!    cannot show.
//! 8. **Failed `try_cast` cleans up and the slot is reusable.** A cast that
//!    does not hold consumes the handle and clears the slot; repeating it
//!    past the budget proves the clear really happened.
//! 9. **A root slot casts to `STObject`.** `from_otxn()` reports a
//!    high-level object code (serialized type ID 10001–10004), not the
//!    ordinary 14, so `try_cast::<STObject>()` must accept it.
//! 10. **An IOU `as_xfl`.** The native round-trip above only exercises one
//!     half of `slot_float`. This reads the sender's *trust line* balance —
//!     a non-native amount — checks `is_native()` reports `false` on it, and
//!     round-trips the value back to the integer the e2e paid in. Together
//!     with check 2 that covers both branches of the amount path live.
//! 11. **A `u64` reads identically through `value()` and raw bytes.** The
//!     typed read decodes the eight wire bytes big-endian rather than using
//!     as-int64 mode, because the host rejects a bit-63 value there and
//!     `sfExchangeRate` legitimately sets it. The account root has no such
//!     field, so this pins the two paths agreeing on a real serialized value;
//!     the bit-63 typing itself is pinned in the trybuild fixtures.
//!
//! Each check contributes one bit to the accept code, so the e2e test can
//! see exactly which passed. Triggered by `Invoke`.
//!
//! # One check group per invocation
//!
//! Every recycling loop runs more than 255 iterations — that is the point,
//! 255 being the per-execution slot budget — and running all of them in one
//! execution needs roughly 130,000 instructions against the Hook API's
//! 65,535 ceiling. So the originating transaction carries a `CHK` parameter
//! naming one group, and the e2e submits one `Invoke` per group and ORs the
//! accept codes. Absent `CHK` runs group 0, everything that needs no loop.
//!
//! Build: `hooks-build build --manifest-path examples/15_slot-objects/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::slot_path;
use hooks_lib::{accept, guard, hook, hook_errors, rollback};

hook_errors! {
    /// `slot-objects` rollback codes.
    pub enum SlotObjectsError {
        /// The originating transaction has no `sfAccount`.
        NoSender = 1,
        /// Building the account keylet failed.
        KeyletFailed = 2,
        /// Loading the sender's account root into a slot failed.
        AccountRootFailed = 3,
    }
}

/// How many iterations each recycling loop runs.
///
/// 260, not a rounder 300: the number only has to exceed the Hook API's
/// 255-slot-per-execution budget, and the guard checker's worst-case
/// accounting sums every loop in the module (a `match` over check groups
/// does not make them alternatives to it), so the total has to stay under
/// the 65,535-instruction ceiling. 260 clears the budget by five slots and
/// leaves room for all five loops.
const LOOP_ITERATIONS: u32 = 260;

/// Bit 0: the account-root walk read all three fields.
const BIT_ACCOUNT_WALK: i64 = 1;
/// Bit 1: `float_int(as_xfl(balance), 6, false)` matched the raw drops.
const BIT_DROPS_ROUNDTRIP: i64 = 2;
/// Bit 2: a child slot stayed readable after its parent was cleared.
const BIT_PARENT_CLEAR: i64 = 4;
/// Bit 3: every `take_value()` in the success loop released its slot.
const BIT_TAKE_LOOP: i64 = 8;
/// Bit 4: 260 `slot_path!` walks that fail on their *third* hop — after two
/// owned intermediates exist — did not exhaust the slots.
const BIT_MIDHOP_LOOP: i64 = 16;
/// Bit 5: 260 *successful* three-hop SignerList walks did not exhaust them.
const BIT_DEEP_LOOP: i64 = 32;
/// Bit 6: 260 *failing* `take_value()` reads did not exhaust them either.
const BIT_TAKE_FAILURE: i64 = 64;
/// Bit 7: 260 failed `try_cast`s cleaned up after themselves.
const BIT_CAST_CLEANUP: i64 = 128;
/// Bit 8: a root slot's high object code is accepted by `try_cast::<STObject>`.
const BIT_ROOT_CAST: i64 = 256;
/// Bit 9: a `u64` field reads identically through `value()` and raw bytes.
const BIT_U64_WIRE: i64 = 512;
/// Bit 10: an IOU trust-line balance reports `is_native() == false` and
/// round-trips through `as_xfl()` to the amount the e2e paid in.
const BIT_IOU_XFL: i64 = 1024;

/// Reads `sfSequence`/`sfAccount`/`sfBalance` off the account root and
/// checks the account matches and the sequence is real.
///
/// `#[inline(never)]` on every check below, and the reason is structural:
/// the Hook API's guard checker rejects a module whose block nesting exceeds
/// 32, and five checks' worth of `if let` ladders inlined into one function
/// reaches 53. Keeping each in its own frame keeps the hook's nesting at a
/// handful — the documented escape hatch (see `docs/DESIGN.md` §5.8 and
/// `examples/81_govern`, which hit the same ceiling).
#[inline(never)]
fn check_account_walk(account: &SlotObject<STObject>, sender: &AccountId) -> bool {
    let Ok(seq): Result<u32> = account.get(sfSequence).and_then(|s| s.value()) else {
        return false;
    };
    let Ok(acc) = account.get(sfAccount).and_then(|s| s.value()) else {
        return false;
    };
    buf_eq_20(&acc, sender) && seq > 0
}

/// Two facts about reading `sfBalance`, checked together because they are
/// the same read seen two ways — and because each extra `#[inline(never)]`
/// frame costs this hook block nesting it cannot spare (see the note below).
///
/// Returns the bits it earned.
///
/// 1. `as_xfl()` on a native amount yields **XAH units**, so
///    `to_int(6, false)` must recover the drop count the raw wire bytes
///    carry.
/// 2. The same eight bytes read as a `u64` must agree between `value()` —
///    which decodes the wire bytes big-endian — and a raw read. The native
///    amount encoding sets bit 63 ("not an IOU"), which is exactly the case
///    the host rejects in as-int64 mode and the reason `u64` does not use
///    it.
#[inline(never)]
fn check_balance_reads(account: &SlotObject<STObject>) -> i64 {
    let Ok(xfl) = account.get(sfBalance).and_then(|s| s.as_xfl()) else {
        return 0;
    };
    let mut buf = [0u8; 8];
    let Ok(_) = account.get(sfBalance).and_then(|s| s.raw(&mut buf)) else {
        return 0;
    };
    let raw = u64::from_be_bytes(buf);
    let Ok(scaled) = xfl.to_int(6, false) else {
        return 0;
    };

    let mut bits: i64 = 0;
    // The wire encoding sets the top "not an IOU" bit; the low 62 bits are
    // the drop count.
    if (scaled as u64) == (raw & 0x3FFF_FFFF_FFFF_FFFF) {
        bits |= BIT_DROPS_ROUNDTRIP;
    }
    // `value()` on a `u64` handle must reproduce those same bytes.
    //
    // On the *shape* of this check: a native amount's top two bits are
    // `0b01` — bit 63 clear means "not an IOU", bit 62 set means positive —
    // so this field does not itself exercise the bit-63 boundary that made
    // `u64::value()` read wire bytes instead of using as-int64 mode. The
    // fields that do (`sfExchangeRate`) live on offers and directories, not
    // on an account root. What this pins is that the two paths agree on a
    // real serialized value, with bit 62 asserted so a run against an
    // all-zero read cannot pass by accident; the typing of the bit-63 path
    // itself is pinned in `tests/ui/pass/slot_typed_api.rs`.
    let typed = account
        .get(sfBalance)
        .map(|s| s.assume_type::<u64>())
        .and_then(|s| s.value());
    if typed == Ok(raw) && (raw >> 62) & 1 == 1 {
        bits |= BIT_U64_WIRE;
    }
    bits
}

/// Derives a child, clears its parent, and only then reads the child — the
/// assumption `slot_path!` is built on.
#[inline(never)]
fn check_parent_clear(keylet: &Keylet) -> bool {
    let Ok(parent) = SlotObject::from_keylet(keylet) else {
        return false;
    };
    let Ok(child) = parent.get(sfSequence) else {
        return false;
    };
    let _ = parent.clear();
    child.value().map(|v: u32| v > 0).unwrap_or(false)
}

/// 260 `slot_path!` walks over the sender's SignerList whose **third** hop
/// fails, after the first two have succeeded.
///
/// Hops 1 and 2 — `sfSignerEntries`, then element 0 — allocate two *owned*
/// intermediates before hop 3 asks a SignerEntry for a `sfBalance` it does
/// not have. That is what makes this a cleanup test: the ladder clears each
/// intermediate unconditionally, before inspecting the result, so 260
/// failures must leak nothing. With a leak the 255-slot budget runs out
/// partway through and the tail fails for a different reason.
///
/// The root is loaded **once** and borrowed by every walk — `slot_path!`
/// never clears its root, and reloading it per iteration would both cost a
/// `slot_set` and obscure what is being recycled.
#[inline(never)]
fn check_midhop_loop(signers: &Keylet) -> bool {
    let Ok(root) = SlotObject::from_keylet(signers) else {
        return false;
    };
    let mut failures: u32 = 0;
    let mut i: u32 = 0;
    while i < LOOP_ITERATIONS {
        guard!(LOOP_ITERATIONS);
        i = i.wrapping_add(1);
        if slot_path!(root[sfSignerEntries][0u32][sfBalance]).is_err() {
            failures = failures.wrapping_add(1);
        }
    }
    let _ = root.clear();
    failures == LOOP_ITERATIONS
}

/// 260 *successful* three-hop walks, each reading its leaf with
/// `take_value()`.
///
/// This one loop proves both success-path contracts at once, which is why
/// there is no separate simple-`take_*` loop: each iteration derives three
/// slots — two intermediates the ladder clears, plus a leaf `take_value()`
/// releases — so 260 iterations move 780 slots through a 255-slot budget.
/// A failure to recycle in *either* mechanism exhausts it well before the
/// end. (The instruction ceiling is 65,535 and the guard checker sums every
/// loop in the module, so folding these together is also what makes room
/// for the failure-path loops below.)
#[inline(never)]
fn check_deep_loop(signers: &Keylet) -> bool {
    let Ok(root) = SlotObject::from_keylet(signers) else {
        return false;
    };
    let mut ok: u32 = 0;
    let mut i: u32 = 0;
    while i < LOOP_ITERATIONS {
        guard!(LOOP_ITERATIONS);
        i = i.wrapping_add(1);
        if let Ok(leaf) = slot_path!(root[sfSignerEntries][0u32][sfAccount]) {
            if leaf.take_value().map(|_: AccountId| true).unwrap_or(false) {
                ok = ok.wrapping_add(1);
            }
        }
    }
    let _ = root.clear();
    ok == LOOP_ITERATIONS
}

/// 260 `take_value()` reads that *fail*, proving `take_*` clears on the
/// failure path as well as the success one.
///
/// `sfSequence` on an account root is a `u32`; reading it as a `Hash` asks
/// for 32 bytes from a 4-byte slot, so every read errors. Only the clear
/// keeps this inside the budget.
#[inline(never)]
fn check_take_failure_loop(keylet: &Keylet) -> bool {
    let Ok(root) = SlotObject::from_keylet(keylet) else {
        return false;
    };
    let mut failures: u32 = 0;
    let mut i: u32 = 0;
    while i < LOOP_ITERATIONS {
        guard!(LOOP_ITERATIONS);
        i = i.wrapping_add(1);
        let derived = root.get(sfSequence).map(|s| s.assume_type::<Hash>());
        if derived.and_then(|s| s.take_value()).is_err() {
            failures = failures.wrapping_add(1);
        }
    }
    let _ = root.clear();
    failures == LOOP_ITERATIONS
}

/// 260 failed `try_cast`s: any failure consumes the handle and best-effort
/// clears the slot, so repeating past the budget must keep failing the same
/// way rather than running out of slots.
#[inline(never)]
fn check_cast_cleanup_loop(keylet: &Keylet) -> bool {
    let Ok(root) = SlotObject::from_keylet(keylet) else {
        return false;
    };
    let mut failures: u32 = 0;
    let mut i: u32 = 0;
    while i < LOOP_ITERATIONS {
        guard!(LOOP_ITERATIONS);
        i = i.wrapping_add(1);
        // `sfSequence` is a UInt32 (type ID 2); casting it to STArray (15)
        // cannot hold.
        if root
            .get(sfSequence)
            .and_then(|s| s.try_cast::<STArray>())
            .is_err()
        {
            failures = failures.wrapping_add(1);
        }
    }
    let _ = root.clear();
    failures == LOOP_ITERATIONS
}

/// The IOU half of the amount path: reads the sender's trust-line balance,
/// which is a **non-native** amount, and checks both that `is_native()` says
/// so and that `as_xfl()` recovers the value the e2e paid in.
///
/// The issuer arrives as an `ISS` parameter on the originating transaction —
/// the e2e generates its wallets at runtime, so the address cannot be baked
/// in. The currency is `USD` in XRPL's standard 20-byte encoding: twelve
/// zero bytes, the three ASCII characters, then five more zeros.
///
/// The comparison uses `to_int(0, true)` — **absolute** value. A
/// `RippleState` object's balance is signed relative to whichever of the two
/// accounts sorts low, which is an ordering this hook has no business
/// depending on; the magnitude is the fact being checked.
#[inline(never)]
fn check_iou_amount(holder: &AccountId) -> bool {
    let Ok(issuer) = otxn_param_exact::<[u8; 20]>(b"ISS") else {
        return false;
    };
    let issuer = AccountId(issuer);
    let mut currency = [0u8; 20];
    // Standard currency code: ASCII at offsets 12..15.
    if let Some(dst) = currency.get_mut(12..15) {
        dst.copy_from_slice(b"USD");
    }
    let Ok(keylet) = keylet_line(holder, &issuer, &CurrencyCode(currency)) else {
        return false;
    };
    let Ok(line) = SlotObject::from_keylet(&keylet) else {
        return false;
    };
    let Ok(balance) = line.get(sfBalance) else {
        return false;
    };
    // Borrowing pre-check, then the consuming read — the composition
    // `is_native` takes `&self` for.
    let Ok(native) = balance.is_native() else {
        return false;
    };
    let Ok(xfl) = balance.as_xfl() else {
        return false;
    };
    let Ok(units) = xfl.to_int(0, true) else {
        return false;
    };
    let _ = line.clear();
    !native && units == IOU_AMOUNT
}

/// How much of the IOU the e2e pays in before invoking, and therefore what
/// [`check_iou_amount`] expects the trust-line balance to round-trip to.
const IOU_AMOUNT: i64 = 100;

/// A root slot reports a high-level object code (serialized type ID
/// 10001–10004), not the ordinary 14 an object *field* reports — so
/// `try_cast::<STObject>()` has to accept it, and the ordinary type IDs
/// must still be rejected.
#[inline(never)]
fn check_root_cast() -> bool {
    let Ok(root) = SlotObject::from_otxn() else {
        return false;
    };
    // The reported code's high half is what the predicate looks at.
    let Ok(code) = root.field_code() else {
        return false;
    };
    let id = code >> 16;
    let is_root_code = (10001..=10004).contains(&id);
    // The cast must accept it...
    let Ok(cast) = root.try_cast::<STObject>() else {
        return false;
    };
    // ...and a wrong target on the same slot must not.
    let rejected = cast.try_cast::<STArray>().is_err();
    is_root_code && rejected
}

/// Which group of checks one invocation runs, read from the `CHK` parameter
/// on the originating transaction.
///
/// **Why the split.** Each recycling loop runs more than 255 iterations —
/// that is the whole point, since 255 is the per-execution slot budget — and
/// a single execution running all five would need ~130,000 instructions
/// against the Hook API's 65,535 limit. So the e2e drives one group per
/// `Invoke` and ORs the accept codes together. Group 0 is everything cheap
/// enough to share an execution.
const CHK_CHEAP: u8 = 0;
/// Group 1: the successful `slot_path!` walk, whose leaf `take_value()`
/// makes it the `take_*` success proof too.
const CHK_DEEP: u8 = 1;
/// Group 2: the `take_*` failure-path loop.
const CHK_TAKE_FAILURE: u8 = 2;
/// Group 3: the failed-`try_cast` cleanup loop.
const CHK_CAST: u8 = 3;
/// Group 4: the `slot_path!` mid-hop-failure loop.
const CHK_MIDHOP: u8 = 4;
/// Group 5: the IOU trust-line read (no loop; it needs the `ISS` parameter
/// the e2e only attaches to this invocation).
const CHK_IOU: u8 = 5;

/// Hook entry point. See the module doc comment for what each bit means.
///
/// Runs the check group named by the `CHK` parameter (absent = group 0) and
/// accepts with the bits it earned.
#[hook]
fn my_hook() -> i64 {
    let sender: AccountId = match otxn_field_exact(sfAccount) {
        Ok(v) => v,
        Err(_) => rollback!(b"slot-objects: no sfAccount", SlotObjectsError::NoSender),
    };
    let keylet = match keylet_account(&sender) {
        Ok(k) => k,
        Err(_) => rollback!(
            b"slot-objects: keylet_account failed",
            SlotObjectsError::KeyletFailed
        ),
    };
    let group: u8 = otxn_param_exact::<[u8; 1]>(b"CHK")
        .map(|v| v[0])
        .unwrap_or(CHK_CHEAP);

    let mut result: i64 = 0;
    match group {
        CHK_DEEP => {
            // Earns both success-path bits: see `check_deep_loop`.
            let signers = keylet_signers(&sender).unwrap_or(Keylet::zeroed());
            if check_deep_loop(&signers) {
                result |= BIT_TAKE_LOOP | BIT_DEEP_LOOP;
            }
        }
        CHK_TAKE_FAILURE => {
            if check_take_failure_loop(&keylet) {
                result |= BIT_TAKE_FAILURE;
            }
        }
        CHK_CAST => {
            if check_cast_cleanup_loop(&keylet) {
                result |= BIT_CAST_CLEANUP;
            }
        }
        CHK_MIDHOP => {
            let signers = keylet_signers(&sender).unwrap_or(Keylet::zeroed());
            if check_midhop_loop(&signers) {
                result |= BIT_MIDHOP_LOOP;
            }
        }
        CHK_IOU => {
            if check_iou_amount(&sender) {
                result |= BIT_IOU_XFL;
            }
        }
        // CHK_CHEAP and anything unrecognized: the checks that need no loop.
        _ => {
            let account = match SlotObject::from_keylet(&keylet) {
                Ok(a) => a,
                Err(_) => rollback!(
                    b"slot-objects: could not slot the account root",
                    SlotObjectsError::AccountRootFailed
                ),
            };
            if check_account_walk(&account, &sender) {
                result |= BIT_ACCOUNT_WALK;
            }
            result |= check_balance_reads(&account);
            if check_parent_clear(&keylet) {
                result |= BIT_PARENT_CLEAR;
            }
            if check_root_cast() {
                result |= BIT_ROOT_CAST;
            }
            let _ = account.clear();
        }
    }

    accept!(b"slot-objects: typed slot layer checks", result)
}
