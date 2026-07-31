//! A per-account deposit ledger using typed state and Hook parameters.
//!
//! `deposit` creates or extends a locked balance; `withdraw` removes it once
//! its lock window expires. The `CFG` parameter configures the minimum amount
//! and lock window, while `ADMIN_PAUSE` can disable new deposits.

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{
    ParamName, accept, hook, hook_errors, hook_parameter, hook_state, otxn_parameter, rollback,
};

/// Discriminant for deposit records.
const DEPOSIT_TAG: u8 = 1;

/// `Instruction::action` value for a deposit.
const ACTION_DEPOSIT: u8 = 1;
/// `Instruction::action` value for a withdrawal.
const ACTION_WITHDRAW: u8 = 2;

/// Default minimum deposit in drops.
const DEFAULT_MIN_AMOUNT: u64 = 1_000_000;
/// Lock window (in ledgers) used when `CFG` isn't configured.
const DEFAULT_LOCK_LEDGERS: u32 = 10;

// Per-account deposit record.
hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32, flags: u8});

// Install-time configuration.
hook_parameter!(Cfg, CfgName = b"CFG" => Config {min_amount: u64, lock_ledgers: u32});

// Per-invocation instruction.
otxn_parameter!(Ins, InsName = b"INS" => Instruction {action: u8, amount: u64});

/// Composite name for administrative parameters.
#[derive(ParamName, Clone, Copy)]
struct AdminName {
    section: u8,
    field: u8,
}

// Administrative pause switch.
hook_parameter!(AdminPause, AdminName => PauseSwitch {paused: u8});

/// Hook-wide deposit pause switch.
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

/// Returns configured values or defaults.
fn config() -> Config {
    Cfg.get_value().unwrap_or(Config {
        min_amount: DEFAULT_MIN_AMOUNT,
        lock_ledgers: DEFAULT_LOCK_LEDGERS,
    })
}

/// Returns whether new deposits are paused.
fn deposits_paused() -> bool {
    ADMIN_PAUSE
        .get_value()
        .map(|s| s.paused != 0)
        .unwrap_or(false)
}

/// Deposit value used when no record exists.
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
            // Delete the state entry to release its owner reserve.
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
