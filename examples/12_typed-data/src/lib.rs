//! `typed-data` — a per-account deposit ledger, keyed by a
//! `#[derive(HookKey)]` struct and valued by a `#[derive(HookData)]` struct,
//! instead of hand-packed byte buffers.
//!
//! Each invocation carries a per-transaction `Instruction` (attached via
//! the Invoke transaction's own `HookParameters`, read with `otxn_param`)
//! naming an action (`deposit`/`withdraw`) and an amount. The hook looks up
//! (or creates) a composite state entry for `{tag, sender}` — a
//! `DepositValue { amount, deadline, flags }` — and updates it:
//!
//! - `deposit`: rejects (rolls back) below the configured minimum; adds the
//!   deposited amount and (re)starts a lock window ending `lock_ledgers`
//!   ledgers from now.
//! - `withdraw`: rejects if there is nothing deposited, or if the lock
//!   window hasn't elapsed yet; otherwise zeroes the entry out.
//!
//! The minimum amount and lock window are themselves a `Config` struct,
//! configurable at install time via the `CFG` Hook parameter (`hook_param`),
//! falling back to a compiled-in default — the same pattern
//! `examples/03_hook-params` uses for a single `u64`, extended here to a
//! multi-field struct.
//!
//! An operator can also pause new deposits entirely, via a **composite**
//! (struct-shaped, not a plain byte-string tag) Hook parameter name —
//! [`AdminName`], a `#[derive(ParamName)]` struct — demonstrating
//! [`hook_parameter!`]'s other grammar form alongside [`CFG_PARAM`]/
//! [`INS_PARAM`]'s plain-tag one. Withdrawals are never paused: a
//! depositor can always get their own money back.
//!
//! Four derives, four roles: [`HookKey`] for the composite state key
//! ([`DepositKey`]), [`HookData`] for the composite state value
//! ([`DepositValue`]), [`ParamName`] for the composite parameter name
//! ([`AdminName`]), and [`ParamValue`] for every parameter payload
//! ([`Config`]/[`Instruction`]/[`PauseSwitch`]) — see each derive's own doc
//! comment (`hooks_lib::{HookKey, HookData, ParamName, ParamValue}`) for why
//! these are four separate, narrower derives rather than one covering
//! everything.
//!
//! See the README for the hand-packed-vs-derived byte layout this replaces,
//! the measured worst-case-instruction-count comparison proving the derive
//! is zero-cost, and why every key/value pair and every named parameter
//! below is declared through [`hook_state!`]/[`hook_parameter!`]/
//! [`otxn_parameter!`] rather than passed as loose, independently-typed
//! arguments at each call site.
//!
//! Build: `hooks-build build --manifest-path examples/12_typed-data/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{
    HookData, HookKey, ParamName, ParamValue, accept, hook, hook_errors, hook_parameter,
    hook_state, otxn_parameter, rollback,
};

/// The one key "kind" this hook stores (reserved for future expansion —
/// see `DepositKey`'s doc comment).
const DEPOSIT_TAG: u8 = 1;

/// Name of the Hook parameter (installed at `SetHook` time) carrying this
/// hook's `Config`. `&[u8; 3]` (a fixed-size array reference, not a bare
/// `&[u8]` slice) so `hook_parameter!`'s two-argument form can infer the
/// generated [`CfgName`] marker's `ToBytes::MAX_LEN` as exactly `3` from it.
const CFG_PARAM: &[u8; 3] = b"CFG";

/// Marker type naming the [`CFG_PARAM`] Hook parameter — see
/// [`hook_parameter!`]'s doc comment for why a plain byte-string name still
/// needs a small marker type declared for it (one Hook API name = one
/// distinct Rust type, so `hook_param_kv`'s argument always resolves
/// [`Config`] unambiguously, even though `CfgName` itself carries no data).
struct CfgName;

/// Name of the Hook parameter attached to each Invoke transaction itself,
/// carrying that invocation's `Instruction`. See [`CFG_PARAM`] for why this
/// is `&[u8; 3]`, not `&[u8]`.
const INS_PARAM: &[u8; 3] = b"INS";

/// Marker type naming the [`INS_PARAM`] Hook parameter — see [`CfgName`]
/// for why.
struct InsName;

/// `Instruction::action` value for a deposit.
const ACTION_DEPOSIT: u8 = 1;
/// `Instruction::action` value for a withdrawal.
const ACTION_WITHDRAW: u8 = 2;

/// Minimum deposit (drops) used when `CFG` isn't configured: 1,000,000
/// drops (1 XAH) — same default `examples/03_hook-params` uses for `MIN`.
const DEFAULT_MIN_AMOUNT: u64 = 1_000_000;
/// Lock window (in ledgers) used when `CFG` isn't configured.
const DEFAULT_LOCK_LEDGERS: u32 = 10;

/// Composite hook-state **key**: which account's deposit record this is.
/// `#[derive(HookKey)]`, not `#[derive(HookData)]` — a key is only ever
/// encoded outward to locate an entry, never read back and decoded as
/// itself (see `hooks_lib::HookKey`'s doc comment).
///
/// `tag` is a constant discriminant (always [`DEPOSIT_TAG`] in this hook) —
/// reserved so a future second record kind could share the same key space
/// without colliding, the same role `state_keys!`'s discriminant byte
/// plays, but expressed as an ordinary struct field instead of an enum
/// variant. See the README's "Before/after" section for the hand-packed
/// 21-byte buffer this replaces.
#[derive(HookKey, Clone, Copy)]
struct DepositKey {
    tag: u8,
    owner: AccountId,
}

/// Composite hook-state **value**: one account's deposit record.
#[derive(HookData, Clone, Copy)]
struct DepositValue {
    /// Drops currently on deposit.
    amount: u64,
    /// Ledger sequence at/after which a withdrawal is allowed (meaningless
    /// while `amount` is `0`).
    deadline: u32,
    /// Bit 0: `1` while a nonzero deposit is outstanding, `0` once
    /// withdrawn (or before any deposit has ever been made).
    flags: u8,
}

// Pairs `DepositKey` with `DepositValue` at the type level (see
// `hooks_lib::state::TypedStateKey`'s doc comment): `state_get_kv`/
// `state_set_kv` below always resolve the value type from the key
// itself, so passing some *other* struct's value for a `DepositKey` is a
// compile error, not a latent bug — the loose `state_get::<T>`/
// `state_set_typed::<T>` (independent `T`) would allow it.
hook_state!(DepositKey => DepositValue);

/// This hook's configuration, installed via the [`CFG_PARAM`] Hook
/// parameter — see [`config`]. `#[derive(ParamValue)]`, not
/// `#[derive(HookData)]` — a parameter value is only ever read back and
/// decoded, never itself used to locate anything (see
/// `hooks_lib::ParamValue`'s doc comment).
#[derive(ParamValue, Clone, Copy)]
struct Config {
    /// Minimum drops a single `deposit` instruction must carry.
    min_amount: u64,
    /// How many ledgers a deposit stays locked for, from the ledger it was
    /// (re)deposited in.
    lock_ledgers: u32,
}

// Pairs `CfgName` with `Config` (see `hooks_lib::convert::TypedParamName`'s
// doc comment): `hook_param_kv(&CfgName)` below reads `CFG_PARAM` and
// decodes the result as `Config` because `CfgName` says so — the name
// argument, not a turbofish or an inferred return type, picks `Config`.
// `hook_parameter!` (not `otxn_parameter!`) because `CfgName` is read via
// `hook_param_kv` (this hook's own installed parameters) below.
hook_parameter!(CfgName, CFG_PARAM => Config);

/// Per-invocation instruction, read from the *originating transaction's
/// own* `HookParameters` (via `otxn_param`, not `hook_param`) — every
/// Invoke that calls this hook attaches its own [`INS_PARAM`], distinct
/// from the hook's installed [`CFG_PARAM`] configuration. `#[derive(ParamValue)]`
/// — see [`Config`]'s doc comment for why.
#[derive(ParamValue)]
struct Instruction {
    /// [`ACTION_DEPOSIT`] or [`ACTION_WITHDRAW`].
    action: u8,
    /// Drops to deposit. Ignored for a withdrawal (which always empties
    /// the whole balance).
    amount: u64,
}

// Same idea as `CfgName`/`Config` above, for the per-transaction `INS`
// parameter — `otxn_parameter!` (not `hook_parameter!`) because `InsName`
// is read via `otxn_param_kv` below.
otxn_parameter!(InsName, INS_PARAM => Instruction);

/// A **composite, struct-shaped** Hook parameter *name* — `{section,
/// field}` — as opposed to [`CFG_PARAM`]/[`INS_PARAM`]'s plain byte-string
/// tags. `#[derive(ParamName)]`, not `#[derive(HookData)]`: a parameter
/// name is only ever written (handed to `hook_param` to locate
/// [`PauseSwitch`] below), never read back and decoded as itself — see
/// `hooks_lib::ParamName`'s doc comment for the full rationale (write-only,
/// no `FromBytes`/`FixedRead`, and the Hook API's 1–32-byte parameter-name
/// bound checked right here at the derive, not only later at a call site).
///
/// `section`/`field` are reserved so future administrative parameters
/// could share this same structured "Admin" name scheme without a naming
/// collision — the same role [`DepositKey`]'s `tag` plays for state keys,
/// applied to a Hook parameter name instead.
#[derive(ParamName, Clone, Copy)]
struct AdminName {
    section: u8,
    field: u8,
}

/// The one [`AdminName`] value naming the "pause switch" parameter below
/// ([`PauseSwitch`]) — `section = 0` reserved for hook-wide administrative
/// controls, `field = 0` the first (and, so far, only) control in that
/// section.
const ADMIN_PAUSE: AdminName = AdminName {
    section: 0,
    field: 0,
};

/// Whether new deposits are currently paused — installed via the
/// composite [`AdminName`] Hook parameter above (`hook_param`, not
/// `otxn_param`: an operator-controlled switch, not something a depositor
/// sets per transaction). See [`deposits_paused`]. `#[derive(ParamValue)]`
/// — see [`Config`]'s doc comment for why.
#[derive(ParamValue, Clone, Copy)]
struct PauseSwitch {
    /// Nonzero: new deposits are rejected. Zero (or the parameter absent
    /// entirely): deposits proceed normally.
    paused: u8,
}

// Pairs the composite `AdminName` with `PauseSwitch` (not a plain byte
// string like `CfgName`/`Config`) — `hook_parameter!`'s composite form,
// `Name => Ty`, mirroring `hook_state!`'s `Key => Value` exactly: the name
// *type* (here, a whole struct) comes first, the type it names comes last.
// Unlike the plain-name form, no literal is baked in here — any `AdminName`
// *value* (see `ADMIN_PAUSE` above) can be passed to `hook_param_kv` at the
// call site, carrying real runtime field data.
hook_parameter!(AdminName => PauseSwitch);

hook_errors! {
    /// `typed-data` rollback codes.
    pub enum TypedDataError {
        /// The originating transaction has no `sfAccount` field (should be
        /// unreachable — every real transaction has one).
        AccountFieldMissing = 1,
        /// The `INS` Hook parameter is missing, or not exactly 9 bytes
        /// (`Instruction`'s encoded length — `#[derive(ParamValue)]` gives
        /// it no inherent `LEN` const to name here, unlike `#[derive(HookData)]`).
        InstructionMissing = 2,
        /// `Instruction::action` is neither [`ACTION_DEPOSIT`] nor
        /// [`ACTION_WITHDRAW`].
        UnknownAction = 3,
        /// A `deposit` instruction's amount fell below [`Config::min_amount`].
        BelowMinimum = 4,
        /// A `withdraw` instruction, but the account has no outstanding
        /// deposit.
        NothingToWithdraw = 5,
        /// A `withdraw` instruction, but the deposit's lock window hasn't
        /// elapsed yet.
        StillLocked = 6,
        /// Reading this account's `DepositValue` failed with something
        /// other than "no entry" (`state`'s `DOESNT_EXIST`).
        StateReadFailed = 7,
        /// Writing the updated `DepositValue` back failed.
        StateSetFailed = 8,
        /// A `deposit` instruction, but the [`AdminName`] pause switch is
        /// currently set. Withdrawals are never rejected for this reason.
        DepositsPaused = 9,
    }
}

/// Reads this hook's [`Config`] from the [`CFG_PARAM`] Hook parameter
/// (named by [`CfgName`]), falling back to [`DEFAULT_MIN_AMOUNT`]/
/// [`DEFAULT_LOCK_LEDGERS`] if it isn't set (or is the wrong size to be a
/// valid `Config`) — the same `hook_param_kv` + `.unwrap_or(..)` pattern
/// `examples/03_hook-params`'s `min_drops` uses for a single `u64` (there,
/// `hook_param_exact`), here reading a whole struct in one call: passing
/// `&CfgName` picks `Config` as the result type (see the `hook_parameter!`
/// declaration above `Config`) — no turbofish, no return-type annotation.
fn config() -> Config {
    hook_param_kv(&CfgName).unwrap_or(Config {
        min_amount: DEFAULT_MIN_AMOUNT,
        lock_ledgers: DEFAULT_LOCK_LEDGERS,
    })
}

/// Reads the [`AdminName`]-named [`PauseSwitch`] Hook parameter, returning
/// whether new deposits are currently paused. Absent (never configured, or
/// the wrong size) is treated as "not paused" — the same
/// `hook_param_kv` + fallback pattern [`config`] uses, here collapsing the
/// result down to a plain `bool` since the caller only needs the one bit.
/// `PauseSwitch` (the closure's inferred parameter type) again comes from
/// the `&ADMIN_PAUSE` argument, not an annotation.
fn deposits_paused() -> bool {
    hook_param_kv(&ADMIN_PAUSE)
        .map(|s| s.paused != 0)
        .unwrap_or(false)
}

/// An all-zero deposit record: no deposit ever made (or one already fully
/// withdrawn).
const EMPTY_DEPOSIT: DepositValue = DepositValue {
    amount: 0,
    deadline: 0,
    flags: 0,
};

/// Hook entry point. See the module doc comment for the full behavior.
#[hook]
fn my_hook() -> i64 {
    let owner: AccountId = match otxn_field_exact(sfAccount) {
        Ok(v) => v,
        Err(_) => rollback!(
            b"typed-data: sfAccount missing from the originating transaction",
            TypedDataError::AccountFieldMissing
        ),
    };

    // `Instruction` (the binding's inferred type) comes from the `&InsName`
    // argument, not an annotation.
    let instruction = match otxn_param_kv(&InsName) {
        Ok(v) => v,
        Err(_) => rollback!(
            b"typed-data: INS parameter missing or malformed",
            TypedDataError::InstructionMissing
        ),
    };

    let key = DepositKey {
        tag: DEPOSIT_TAG,
        owner,
    };

    let current = match state_get_kv(&key) {
        Ok(existing) => existing.unwrap_or(EMPTY_DEPOSIT),
        Err(_) => rollback!(
            b"typed-data: state read failed",
            TypedDataError::StateReadFailed
        ),
    };

    let cfg = config();

    let next = match instruction.action {
        ACTION_DEPOSIT => {
            if deposits_paused() {
                rollback!(
                    b"typed-data: deposits are currently paused",
                    TypedDataError::DepositsPaused
                );
            }
            if instruction.amount < cfg.min_amount {
                rollback!(
                    b"typed-data: deposit below configured minimum",
                    TypedDataError::BelowMinimum
                );
            }
            DepositValue {
                amount: current.amount.wrapping_add(instruction.amount),
                deadline: ledger_seq().wrapping_add(cfg.lock_ledgers),
                flags: 1,
            }
        }
        ACTION_WITHDRAW => {
            if current.flags == 0 {
                rollback!(
                    b"typed-data: nothing to withdraw",
                    TypedDataError::NothingToWithdraw
                );
            }
            if ledger_seq() < current.deadline {
                rollback!(
                    b"typed-data: deposit still locked",
                    TypedDataError::StillLocked
                );
            }
            EMPTY_DEPOSIT
        }
        _ => rollback!(
            b"typed-data: unknown INS action",
            TypedDataError::UnknownAction
        ),
    };

    if state_set_kv(&key, &next).is_err() {
        rollback!(
            b"typed-data: state_set failed",
            TypedDataError::StateSetFailed
        );
    }

    accept!(b"typed-data: ok", next.amount as i64)
}
