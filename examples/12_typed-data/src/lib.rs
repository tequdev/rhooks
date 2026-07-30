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
//!   window hasn't elapsed yet; otherwise **deletes** the entry, which
//!   refunds the owner reserve it was holding (an all-zero entry left in
//!   place would keep both the ledger object and the reserve).
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
//! lets each access be written with no turbofish and no chance of a
//! key/value or name/value mismatch.
//!
//! Those declared types also carry their own accessors, which is how this
//! hook reads and writes: `deposit.get_state()`/`deposit.set_state(&v)` for
//! state, `Cfg.get_value()` for a parameter. Each is the method form of
//! `state_get_typed(&deposit)`/`state_set_typed(&deposit, &v)`/
//! `hook_param_typed(&CfgName)`/`otxn_param_typed(&InsName)`,
//! `#[inline(always)]` onto the identical code — the README's measured
//! instruction counts did not move when this hook switched to them.
//!
//! See the README for the hand-packed byte layout this replaces and the
//! measured worst-case-instruction-count comparison proving it zero-cost,
//! and `hooks_lib::hook_state!`'s doc comment for the full grammar
//! staircase this example exercises three steps of: **Form 1**
//! (`Cfg`/`Ins`, fully fixed names), **Form 3** (`DepositState`,
//! struct-shaped, constructed per call site), and the **pairing form**
//! (`AdminPause` wrapping the separately-derived `AdminName`, whose
//! `with_name_bytes` the entity forwards to rather than re-deriving).
//!
//! Build: `hooks-build build --manifest-path examples/12_typed-data/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{
    ParamName, accept, hook, hook_errors, hook_parameter, hook_state, otxn_parameter, rollback,
};

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
// with an **inline** value definition, declaring in one line what a
// `#[derive(HookKey)] struct DepositKey`, a
// `#[derive(HookData)] struct DepositValue`, and a separate
// `hook_state!(DepositState, DepositKey => DepositValue)` pairing would
// otherwise take.
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
hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32, flags: u8});

// This hook's configuration, installed via the `CFG` Hook parameter — see
// [`config`]. `hook_parameter!`'s **Form 1** (a fully fixed name): one line
// declares the `Cfg` entity, the zero-sized `CfgName` key component, and —
// **inline** — the `Config` value with its two fields.
//
// `min_amount` — minimum drops a single `deposit` instruction must carry;
// `lock_ledgers` — how many ledgers a deposit stays locked for, from the
// ledger it was (re)deposited in.
hook_parameter!(Cfg, CfgName = b"CFG" => Config {min_amount: u64, lock_ledgers: u32});

// Per-invocation instruction, read from the *originating transaction's own*
// `HookParameters` (via `otxn_param`, not `hook_param`) — every Invoke that
// calls this hook attaches its own `INS` parameter, distinct from the hook's
// installed `CFG` configuration. `otxn_parameter!`'s **Form 1**, exactly
// like [`Cfg`] above but read via `otxn_param` instead of `hook_param`:
// which of the two an entity reads is fixed by the macro that declared it.
//
// `action` — [`ACTION_DEPOSIT`] or [`ACTION_WITHDRAW`]; `amount` — drops to
// deposit (ignored for a withdrawal, which always empties the whole
// balance).
otxn_parameter!(Ins, InsName = b"INS" => Instruction {action: u8, amount: u64});

/// A **composite, struct-shaped** Hook parameter *name* — `{section,
/// field}` — as opposed to `CFG`/`INS`'s plain byte-string tags.
///
/// `section`/`field` are reserved so future administrative parameters could
/// share this same structured "Admin" name scheme without a naming
/// collision — the same role `DepositKey`'s `tag` plays for state keys,
/// applied to a Hook parameter name instead.
///
/// Declared with `#[derive(ParamName)]` and paired below, rather than
/// inline like `CfgName`, to show the **pairing form**: a name type the
/// caller already owns, wrapped by an entity that forwards to it. See the
/// declaration below for what that forwarding costs (nothing).
#[derive(ParamName, Clone, Copy)]
struct AdminName {
    section: u8,
    field: u8,
}

// `hook_parameter!`'s **pairing form** (`Entity, Key => Value`): `AdminName`
// is already a `ToBytes`/`TypedParamName`-capable name above, so this line
// declares only the `AdminPause` entity — `struct AdminPause(AdminName);` —
// plus an inline `PauseSwitch` value (`paused` nonzero means new deposits
// are rejected; zero, or the parameter absent entirely, means deposits
// proceed normally).
//
// The entity forwards `TypedParamName::with_name_bytes` straight to
// `AdminName`'s own implementation rather than re-deriving one, so the
// exact-size (2-byte) scratch buffer that name already had is what the
// lookup uses — measured identical to naming it inline (see the README's
// "Measured cost of a composite name" section).
hook_parameter!(AdminPause, AdminName => PauseSwitch {paused: u8});

/// The one [`AdminPause`] value naming the "pause switch" parameter above —
/// `section = 0` reserved for hook-wide administrative controls, `field =
/// 0` the first (and, so far, only) control in that section.
const ADMIN_PAUSE: AdminPause = AdminPause(AdminName {
    section: 0,
    field: 0,
});

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
        /// Writing the updated `DepositValue` back — or, on a full
        /// withdrawal, deleting it — failed.
        StateSetFailed = 8,
        /// A `deposit` instruction, but the [`AdminName`] pause switch is
        /// currently set. Withdrawals are never rejected for this reason.
        DepositsPaused = 9,
    }
}

/// Reads this hook's [`Config`] from the `CFG` Hook parameter (named by
/// [`CfgName`]), falling back to [`DEFAULT_MIN_AMOUNT`]/
/// [`DEFAULT_LOCK_LEDGERS`] if it isn't set (or is the wrong size to be a
/// valid `Config`) — the same "read the parameter, `.unwrap_or(..)` a
/// default" pattern `examples/03_hook-params`'s `min_drops` uses for a
/// single `u64` (there, `hook_param_exact`), here reading a whole struct in
/// one call: `Cfg.get_value()` resolves `Config` from the entity it is
/// called on (see the `hook_parameter!` declaration above `Config`) — no
/// turbofish, no return-type annotation. The method is
/// `hook_param_typed(&CfgName)` inlined; either spelling compiles to the
/// same thing.
fn config() -> Config {
    Cfg.get_value().unwrap_or(Config {
        min_amount: DEFAULT_MIN_AMOUNT,
        lock_ledgers: DEFAULT_LOCK_LEDGERS,
    })
}

/// Reads the [`AdminName`]-named [`PauseSwitch`] Hook parameter, returning
/// whether new deposits are currently paused. Absent (never configured, or
/// the wrong size) is treated as "not paused" — the same read-plus-fallback
/// pattern [`config`] uses, here collapsing the result down to a plain
/// `bool` since the caller only needs the one bit. `PauseSwitch` (the
/// closure's inferred parameter type) again comes from the name
/// `get_value` was called on, not an annotation — and note that
/// `AdminName` is a *composite* name, so this is the encode-into-an
/// exact-size-buffer path, not the `'static`-literal one `CfgName` takes.
fn deposits_paused() -> bool {
    ADMIN_PAUSE.get_value().map(|s| s.paused != 0).unwrap_or(false)
}

/// An all-zero deposit record: what a *read* decodes to when the account
/// has no entry at all — either because it never deposited, or because a
/// full withdrawal deleted the entry (see [`my_hook`]'s withdraw branch;
/// this value is never stored).
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

    // `Instruction` (the binding's inferred type) comes from the entity
    // `get_value` is called on, not an annotation.
    let instruction = match Ins.get_value() {
        Ok(v) => v,
        Err(_) => rollback!(
            b"typed-data: INS parameter missing or malformed",
            TypedDataError::InstructionMissing
        ),
    };

    let deposit = DepositState {
        tag: DEPOSIT_TAG,
        owner,
    };

    let current = match deposit.get_state() {
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
            // A withdrawal always empties the whole balance, so the record
            // is **deleted**, not overwritten with [`EMPTY_DEPOSIT`]: an
            // all-zero entry still occupies a ledger object and still holds
            // the owner reserve it was created with, while a deletion (a
            // zero-length `state_set`, which `delete_state` is the typed
            // spelling of) frees both. `EMPTY_DEPOSIT` remains the value a
            // *read* of a now-absent entry decodes to, above.
            //
            // This is the one path that cannot go through `set_state`: no
            // `DepositValue` means "remove this" — see
            // `hooks_lib::state::state_delete`'s doc comment.
            if deposit.delete_state().is_err() {
                rollback!(
                    b"typed-data: state_set failed",
                    TypedDataError::StateSetFailed
                );
            }
            accept!(b"typed-data: ok", 0)
        }
        _ => rollback!(
            b"typed-data: unknown INS action",
            TypedDataError::UnknownAction
        ),
    };

    if deposit.set_state(&next).is_err() {
        rollback!(
            b"typed-data: state_set failed",
            TypedDataError::StateSetFailed
        );
    }

    accept!(b"typed-data: ok", next.amount as i64)
}
