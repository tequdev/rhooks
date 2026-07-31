#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, hook_parameter, rollback};

hook_parameter!(BlockedParam, BlockedParamName = b"BL" => AccountId);

hook_errors! {
    /// Errors returned by the firewall hook.
    pub enum FirewallError {
        /// The originating account could not be read.
        CouldNotReadSender = 1,
        /// The originating account is blocked.
        BlockedAccount = 2,
    }
}

#[hook]
fn my_hook() -> i64 {
    let mut sender = AccountId::default();
    match otxn_field(&mut sender, sfAccount) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => rollback!(
            b"firewall: could not read otxn sender",
            FirewallError::CouldNotReadSender
        ),
    }

    let blocked: AccountId = match BlockedParam.get_value() {
        Ok(v) => v,
        Err(_) => accept!(),
    };

    // Avoid `==`, which can compile to an unguarded loop.
    if buf_eq_20(&sender, &blocked) {
        rollback!(b"firewall: blocked account", FirewallError::BlockedAccount);
    }

    accept!()
}
