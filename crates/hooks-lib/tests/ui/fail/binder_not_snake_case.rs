//! A binder in camelCase: it names a local variable, so it must be
//! snake_case (rustc would otherwise warn `non_snake_case` from inside an
//! expansion the caller cannot see).

use hooks_lib::hook_parameter;

fn main() {
    hook_parameter!(acctParam, AcctParamName = b"ACCT" => [u8; 20]);
}
