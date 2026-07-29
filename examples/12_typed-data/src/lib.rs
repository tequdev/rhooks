//! `typed-data` — a per-account deposit ledger, keyed and valued through
//! `hook_state!`'s declaration-macro grammar instead of hand-packed byte
//! buffers.
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
//! [`AdminName`] — demonstrating `hook_parameter!`'s Form 3 (a struct name
//! constructed per call site) alongside `CFG`/`INS`'s Form 1 (a fully fixed
//! name). Withdrawals are never paused: a depositor can always get their
//! own money back.
//!
//! Every key/value and name/value pair below is declared through
//! [`hook_state!`]/[`hook_parameter!`]/[`otxn_parameter!`] in one line each
//! — the key/name struct, its value struct (declared *inline*, right in the
//! same declaration — `=> Name { field: Type, .. }`), and the pairing that
//! lets `state_get_typed`/`state_set_typed`/`hook_param_typed`/
//! `otxn_param_typed` be called with no turbofish and no chance of a
//! key/value or name/value mismatch. See the README for the hand-packed
//! byte layout this replaces and the measured worst-case-instruction-count
//! comparison proving it zero-cost, and `hooks_lib::hook_state!`'s doc
//! comment for the full grammar staircase (Forms 1–4 plus the
//! backward-compatible existing-types form) this example exercises three
//! steps of: Form 1 (`CfgName`/`InsName`, fully fixed names), Form 3
//! (`DepositKey`/`AdminName`, struct-shaped, constructed per call site).
//!
//! Build: `hooks-build build --manifest-path examples/12_typed-data/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, hook_parameter, hook_state, otxn_parameter, rollback};

/// The one key "kind" this hook stores (reserved for future expansion —
/// see `DepositKey`'s doc comment).
const DEPOSIT_TAG: u8 = 1;

/// `Instruction::action` value for a deposit.
const ACTION_DEPOSIT: u8 = 1;
/// `Instruction::action` value for a withdrawal.
const ACTION_WITHDRAW: u8 = 2;

/// Minimum deposit (drops) used when `CFG` isn't configured: 1,000,000
/// drops (1 XAH) — same default `examples/03_hook-params` uses for `MIN`.
const DEFAULT_MIN_AMOUNT: u64 = 1_000_000;
/// Lock window (in ledgers) used when `CFG` isn't configured.
const DEFAULT_LOCK_LEDGERS: u32 = 10;

// Composite hook-state key/value pair: which account's deposit record
// this is, and the record itself. `hook_state!`'s **Form 3** (a
// struct-shaped key, constructed at each call site — see `my_hook` below)
// with an **inline** value definition, declaring in one line what used to
// take a `#[derive(HookKey)] struct DepositKey`, a
// `#[derive(HookData)] struct DepositValue`, and a separate
// `hook_state!(DepositKey => DepositValue)` pairing.
//
// `tag` is a constant discriminant (always [`DEPOSIT_TAG`] in this hook) —
// reserved so a future second record kind could share the same key space
// without colliding, the same role `state_keys!`'s discriminant byte
// plays, but expressed as an ordinary struct field instead of an enum
// variant. See the README's "Before/after" section for the hand-packed
// 21-byte buffer this replaces.
//
// `DepositValue`'s fields: `amount` — drops currently on deposit;
// `deadline` — the ledger sequence at/after which a withdrawal is
// allowed (meaningless while `amount` is `0`); `flags` — bit 0 is `1`
// while a nonzero deposit is outstanding, `0` once withdrawn (or before
// any deposit has ever been made).
hook_state!(DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32, flags: u8});

// This hook's configuration, installed via the `CFG` Hook parameter — see
// [`config`]. `hook_parameter!`'s **Form 1** (a fully fixed name — a new
// zero-sized `CfgName` type, declared by this one line) with an **inline**
// value definition (`Config`'s two fields).
//
// `min_amount` — minimum drops a single `deposit` instruction must carry;
// `lock_ledgers` — how many ledgers a deposit stays locked for, from the
// ledger it was (re)deposited in.
hook_parameter!(CfgName = b"CFG" => Config {min_amount: u64, lock_ledgers: u32});

// Per-invocation instruction, read from the *originating transaction's
// own* `HookParameters` (via `otxn_param`, not `hook_param`) — every
// Invoke that calls this hook attaches its own `INS` parameter, distinct
// from the hook's installed `CFG` configuration. `otxn_parameter!`'s
// **Form 1**, exactly like [`Config`] above but read via
// `otxn_param_typed` instead of `hook_param_typed`.
//
// `action` — [`ACTION_DEPOSIT`] or [`ACTION_WITHDRAW`]; `amount` — drops
// to deposit (ignored for a withdrawal, which always empties the whole
// balance).
otxn_parameter!(InsName = b"INS" => Instruction {action: u8, amount: u64});

// A **composite, struct-shaped** Hook parameter *name* — `{section,
// field}` — as opposed to `CFG`/`INS`'s plain byte-string tags.
// `hook_parameter!`'s **Form 3** (struct-shaped name, constructed per call
// site — see [`ADMIN_PAUSE`] below) with an inline `PauseSwitch` value:
// `paused` nonzero means new deposits are rejected; zero (or the
// parameter absent entirely) means deposits proceed normally.
//
// `section`/`field` are reserved so future administrative parameters
// could share this same structured "Admin" name scheme without a naming
// collision — the same role `DepositKey`'s `tag` plays for state keys,
// applied to a Hook parameter name instead.
hook_parameter!(AdminName {section: u8, field: u8} => PauseSwitch {paused: u8});

/// The one [`AdminName`] value naming the "pause switch" parameter above —
/// `section = 0` reserved for hook-wide administrative controls, `field =
/// 0` the first (and, so far, only) control in that section.
const ADMIN_PAUSE: AdminName = AdminName {
    section: 0,
    field: 0,
};

hook_errors! {
    /// `typed-data` rollback codes.
    pub enum TypedDataError {
        /// The originating transaction has no `sfAccount` field (should be
        /// unreachable — every real transaction has one).
        AccountFieldMissing = 1,
        /// The `INS` Hook parameter is missing, or not exactly 9 bytes
        /// (`Instruction`'s encoded length).
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

/// Reads this hook's [`Config`] from the `CFG` Hook parameter (named by
/// [`CfgName`]), falling back to [`DEFAULT_MIN_AMOUNT`]/
/// [`DEFAULT_LOCK_LEDGERS`] if it isn't set (or is the wrong size to be a
/// valid `Config`) — the same `hook_param_typed` + `.unwrap_or(..)` pattern
/// `examples/03_hook-params`'s `min_drops` uses for a single `u64` (there,
/// `hook_param_exact`), here reading a whole struct in one call: passing
/// `&CfgName` picks `Config` as the result type (see the `hook_parameter!`
/// declaration above `Config`) — no turbofish, no return-type annotation.
fn config() -> Config {
    hook_param_typed(&CfgName).unwrap_or(Config {
        min_amount: DEFAULT_MIN_AMOUNT,
        lock_ledgers: DEFAULT_LOCK_LEDGERS,
    })
}

/// Reads the [`AdminName`]-named [`PauseSwitch`] Hook parameter, returning
/// whether new deposits are currently paused. Absent (never configured, or
/// the wrong size) is treated as "not paused" — the same
/// `hook_param_typed` + fallback pattern [`config`] uses, here collapsing the
/// result down to a plain `bool` since the caller only needs the one bit.
/// `PauseSwitch` (the closure's inferred parameter type) again comes from
/// the `&ADMIN_PAUSE` argument, not an annotation.
fn deposits_paused() -> bool {
    hook_param_typed(&ADMIN_PAUSE)
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
    let instruction = match otxn_param_typed(&InsName) {
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

    let current = match state_get_typed(&key) {
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

    if state_set_typed(&key, &next).is_err() {
        rollback!(
            b"typed-data: state_set failed",
            TypedDataError::StateSetFailed
        );
    }

    accept!(b"typed-data: ok", next.amount as i64)
}
