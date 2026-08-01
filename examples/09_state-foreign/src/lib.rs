#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::*;

metadata! {
    name: "state-foreign",
    HookOn: [Invoke],
}

hook_parameter!(AcctParam, AcctParamName = b"ACCT" => AccountId);

/// The right-padded key for the target account's flag.
const ENABLED_KEY: StateKey = StateKey(pad!(b"enabled"));

hook_errors! {
    /// Errors returned by the foreign-state hook.
    pub enum StateForeignError {
        /// The target account is not configured.
        AcctNotConfigured = 1,
        /// The target has no flag in this hook's namespace.
        NotConfiguredOnTarget = 2,
        /// Reading the target flag failed.
        ReadFailed = 3,
        /// The target flag is disabled.
        FlagOff = 4,
    }
}

#[hook]
fn my_hook() -> i64 {
    let Ok(target) = AcctParam.get_value() else {
        rollback!(
            b"state-foreign: ACCT parameter not configured",
            StateForeignError::AcctNotConfigured
        )
    };

    let mut flag = [0u8; 1];
    match state_foreign(&mut flag, &ENABLED_KEY, None, &target) {
        Ok(n) if n == flag.len() => {}
        Err(HookError::DoesntExist) => rollback!(
            b"state-foreign: not configured on target account",
            StateForeignError::NotConfiguredOnTarget
        ),
        _ => rollback!(
            b"state-foreign: state_foreign read failed",
            StateForeignError::ReadFailed
        ),
    }

    if flag[0] == 0 {
        rollback!(
            b"state-foreign: target account's flag is off",
            StateForeignError::FlagOff
        );
    }

    accept!()
}
