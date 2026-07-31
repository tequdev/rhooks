#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::*;

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
    let Ok(sender) = otxn_field_typed(sfAccount) else {
        rollback!(
            b"firewall: could not read otxn sender",
            FirewallError::CouldNotReadSender
        )
    };

    let Ok(blocked) = BlockedParam.get_value() else {
        accept!()
    };

    // Avoid `==`, which can compile to an unguarded loop.
    if buf_eq_20(&sender, &blocked) {
        rollback!(b"firewall: blocked account", FirewallError::BlockedAccount);
    }

    accept!()
}
