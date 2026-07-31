#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::*;

hook_parameter!(BlockedParam, BlockedParamName = b"BL" => AccountId);

hook_errors! {
    /// Errors returned by the guarded firewall hook.
    pub enum GuardPatternsError {
        /// The originating account could not be read.
        CouldNotReadSender = 1,
        /// The originating account is blocked.
        BlockedAccount = 2,
    }
}

#[hook]
// Keep the loops on one line to demonstrate `guard_m!` disambiguators.
#[rustfmt::skip]
fn my_hook() -> i64 {
    let sender: AccountId = match otxn_field_exact(sfAccount) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"guard-patterns: could not read otxn sender",
            GuardPatternsError::CouldNotReadSender
        ),
    };

    let Ok(blocked) = BlockedParam.get_value() else {
        accept!();
    };

    if buf_eq_20(&sender, &blocked) {
        rollback!(
            b"guard-patterns: blocked account",
            GuardPatternsError::BlockedAccount
        );
    }

    let mut i: usize = 0; let mut sum_a: u32 = 0; loop { guard_m!(8, 1); if i >= 8 { break; } sum_a = sum_a.wrapping_add(u32::from(sender.get(i).copied().unwrap_or(0))); i = i.wrapping_add(1); }
    let mut j: usize = 0; let mut sum_b: u32 = 0; loop { guard_m!(8, 2); if j >= 8 { break; } sum_b = sum_b.wrapping_add(u32::from(blocked.get(j).copied().unwrap_or(0))); j = j.wrapping_add(1); }

    accept!(b"guard-patterns: accepted", i64::from(sum_a.wrapping_add(sum_b)))
}
