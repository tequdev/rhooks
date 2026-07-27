//! `typed-data` — a per-account deposit ledger, keyed and valued by
//! `#[derive(HookData)]` structs instead of hand-packed byte buffers.
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
    HookData, accept, hook, hook_errors, hook_parameter, hook_state, otxn_parameter, rollback,
};

/// The one key "kind" this hook stores (reserved for future expansion —
/// see `DepositKey`'s doc comment).
const DEPOSIT_TAG: u8 = 1;

/// Name of the Hook parameter (installed at `SetHook` time) carrying this
/// hook's `Config`. `&[u8; 3]` (a fixed-size array reference, not a bare
/// `&[u8]` slice) so `hook_parameter!`'s simple form can infer
/// `ParamName::Name` as `[u8; 3]` from it.
const CFG_PARAM: &[u8; 3] = b"CFG";

/// Name of the Hook parameter attached to each Invoke transaction itself,
/// carrying that invocation's `Instruction`. See [`CFG_PARAM`] for why this
/// is `&[u8; 3]`, not `&[u8]`.
const INS_PARAM: &[u8; 3] = b"INS";

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
///
/// `tag` is a constant discriminant (always [`DEPOSIT_TAG`] in this hook) —
/// reserved so a future second record kind could share the same key space
/// without colliding, the same role `state_keys!`'s discriminant byte
/// plays, but expressed as an ordinary struct field instead of an enum
/// variant. See the README's "Before/after" section for the hand-packed
/// 21-byte buffer this replaces.
#[derive(HookData, Clone, Copy)]
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
/// parameter — see [`config`].
#[derive(HookData, Clone, Copy)]
struct Config {
    /// Minimum drops a single `deposit` instruction must carry.
    min_amount: u64,
    /// How many ledgers a deposit stays locked for, from the ledger it was
    /// (re)deposited in.
    lock_ledgers: u32,
}

// Ties `Config` to its own parameter name (see `hooks_lib::convert::ParamName`'s
// doc comment): `hook_param_kv::<Config>()` below reads `CFG_PARAM` because
// `Config` says so, not because some call site happened to pass the right
// byte string for the right type. `hook_parameter!` (not `otxn_parameter!`)
// because `Config` is read via `hook_param_kv` (this hook's own installed
// parameters) below.
hook_parameter!(Config => CFG_PARAM);

/// Per-invocation instruction, read from the *originating transaction's
/// own* `HookParameters` (via `otxn_param`, not `hook_param`) — every
/// Invoke that calls this hook attaches its own [`INS_PARAM`], distinct
/// from the hook's installed [`CFG_PARAM`] configuration.
#[derive(HookData)]
struct Instruction {
    /// [`ACTION_DEPOSIT`] or [`ACTION_WITHDRAW`].
    action: u8,
    /// Drops to deposit. Ignored for a withdrawal (which always empties
    /// the whole balance).
    amount: u64,
}

// Same idea as `Config` above, for the per-transaction `INS` parameter —
// `otxn_parameter!` (not `hook_parameter!`) because `Instruction` is read
// via `otxn_param_kv` below.
otxn_parameter!(Instruction => INS_PARAM);

hook_errors! {
    /// `typed-data` rollback codes.
    pub enum TypedDataError {
        /// The originating transaction has no `sfAccount` field (should be
        /// unreachable — every real transaction has one).
        AccountFieldMissing = 1,
        /// The `INS` Hook parameter is missing, or not exactly
        /// [`Instruction::LEN`] bytes.
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
    }
}

/// Reads this hook's [`Config`] from the [`CFG_PARAM`] Hook parameter,
/// falling back to [`DEFAULT_MIN_AMOUNT`]/[`DEFAULT_LOCK_LEDGERS`] if it
/// isn't set (or is the wrong size to be a valid `Config`) — the same
/// `hook_param_kv` + `.unwrap_or(..)` pattern
/// `examples/03_hook-params`'s `min_drops` uses for a single `u64` (there,
/// `hook_param_exact`), here reading a whole struct in one call with no
/// `CFG_PARAM` argument at the call site at all (see the `hook_parameter!`
/// declaration above `Config`).
fn config() -> Config {
    hook_param_kv().unwrap_or(Config {
        min_amount: DEFAULT_MIN_AMOUNT,
        lock_ledgers: DEFAULT_LOCK_LEDGERS,
    })
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

    let instruction: Instruction = match otxn_param_kv() {
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
