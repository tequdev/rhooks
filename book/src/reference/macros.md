# Macro Reference

A lookup table of every user-facing macro, derive, and attribute `rshooks`
exports. Each entry is a one-line purpose plus a minimal invocation sketch —
for the full grammar, worked examples, and edge cases, follow the link into
the tutorial chapter that covers it, or the macro's own rustdoc in
`crates/rshooks/src/lib.rs`.

## Entry points

| macro | purpose | sketch |
|---|---|---|
| `#[hook]` | Turns a plain `fn name() -> i64` into the required wasm `hook` export. | `#[hook] fn my_hook() -> i64 { accept!() }` |
| `#[cbak]` | Same as `#[hook]`, but exports `cbak` — the optional callback invoked when a transaction this hook emitted later settles. | `#[cbak] fn my_cbak() -> i64 { accept!() }` |

See [Anatomy of a Hook](../concepts/anatomy.md) and [Emitting
Transactions](../emit/emitting.md).

## Control flow & exit

| macro | purpose | sketch |
|---|---|---|
| `accept!` | Terminate successfully, optionally with a message and code. | `accept!()` / `accept!(b"ok", 0)` |
| `rollback!` | Terminate with failure, rolling back state changes. | `rollback!(b"blocked", FirewallError::BlockedAccount)` |
| `guard!` | Bound a loop's iteration count for the host's static guard check. | `loop { guard!(10); .. }` |
| `guard_m!` | Like `guard!`, for multiple loops sharing one source line (`$n` disambiguates). | `guard_m!(10, 0);` |
| `hook_errors!` | Declare a `#[repr(i64)]` error enum usable directly as a `rollback!`/`accept!` code. | `hook_errors! { pub enum E { BlockedAccount = 1 } }` |
| `exit_on_err!` | Unwrap a `Result<T, E: Into<i64>>`, rolling back on `Err`. | `let v = exit_on_err!(b"failed", check());` |

See [Accept, Rollback, and Errors](../concepts/errors.md) and [Guards and
Loops](../concepts/guards.md).

## Data & typing

| macro | purpose | sketch |
|---|---|---|
| `#[derive(HookData)]` | Encode/decode a fixed-size, named-field struct as a **hook-state value** (or a parameter value, or a nested field). | `#[derive(HookData)] struct Deposit { amount: u64 }` |
| `#[derive(HookKey)]` | Encode a fixed-size, named-field struct as a **hook-state key** (encode-only, 32-byte bound checked at derive time). | `#[derive(HookKey)] struct DepositKey { tag: u8, owner: AccountId }` |
| `#[derive(ParamName)]` | Encode a fixed-size, named-field struct as a **Hook API parameter name** (encode-only, 1–32-byte bound checked at derive time). | `#[derive(ParamName)] struct SeatParamName { topic: u8, seat: u8 }` |
| `#[derive(ParamValue)]` | Decode a fixed-size, named-field struct as a **Hook API parameter value** (decode-only). | `#[derive(ParamValue)] struct Config { min_amount: u64 }` |
| `hook_state!` | Declare a hook-state entity — key + value pairing, with `get_state`/`set_state`/`update_state`/`delete_state` accessors. Six grammar forms. | `hook_state!(DepositState, DepositKey {tag: u8} => Deposit {amount: u64});` |
| `state_keys!` | Declare an enum of hook-state keys, each variant its own real byte length. | `state_keys! { enum DataKey { Counter, Balance(AccountId) } }` |
| `hook_parameter!` | Declare a Hook API parameter (this hook's own installed parameters) — name + value pairing, `get_value` (and `get_name` for byte-string names). Same grammar staircase as `hook_state!`. | `hook_parameter!(Cfg, CfgName = b"CFG" => Config);` |
| `otxn_parameter!` | Identical to `hook_parameter!`, but reads the *originating transaction's* parameters via `otxn_param_typed`. | `otxn_parameter!(Ins, InsName = b"INS" => Instruction);` |

See [Hook State](../data/state.md), [Hook and Transaction
Parameters](../data/parameters.md), and [Typed Data with
Derives](../data/typed-data.md).

## Compile-time literals

| macro | purpose | sketch |
|---|---|---|
| `XFL!` | Encode a decimal numeric literal into a bit-exact `xfl::XFL` value at compile time (never via `f64`). | `const RATE: XFL = XFL!(0.003333333333333333);` |
| `account_id!` | Decode a classic r-address into an `AccountId` at compile time. | `const OWNER: AccountId = account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh");` |

See [XFL: Decimal Floating Point](../data/xfl.md) and
[Keylets](../data/keylets.md) (which covers `account_id!`).

## Transactions

| macro | purpose | sketch |
|---|---|---|
| `txn_template!` | Declare a typed, byte-exact emitted-transaction template: field list in, `new()`/setters/`prepare_for_emit()`/`emit()` out. | `txn_template! { struct Payment { transaction_type = ttPAYMENT, .. } }` |

See [Emitting Transactions](../emit/emitting.md).

## Slots

| macro | purpose | sketch |
|---|---|---|
| `slot_path!` | Walk a multi-hop `SlotObject` path, clearing each intermediate handle as soon as its child exists — no `?`-chain slot leaks. | `slot_path!(root[sfSigners][0][sfAccount])` |

See [Slots and Ledger Objects](../data/slots.md).

## Tracing & buffers

| macro | purpose | sketch |
|---|---|---|
| `trace!` | Emit a debug trace message (and optional byte payload). Compiles to nothing unless the `trace` feature is enabled. | `trace!(b"checkpoint");` |
| `trace_num!` | Emit a trace message followed by an integer. | `trace_num!(b"count", count);` |
| `trace_float!` | Emit a trace message followed by an XFL value. | `trace_float!(b"rate", rate);` |
| `pad!` | Zero-pad a constant byte string to a fixed-size array at compile time, `src` at the front. | `const KEY: StateKey = StateKey(pad!(b"counter"));` |
| `pad_left!` | Same as `pad!`, but `src` at the end (zero bytes first). | `const KEY: StateKey = StateKey(pad_left!(b"counter"));` |

See [Tracing and Debugging](../concepts/tracing.md) and [Hook
State](../data/state.md).

## Build metadata

| macro | purpose | sketch |
|---|---|---|
| `metadata!` | Declare a Hook's descriptive/SetHook-facing metadata (name, `HookOn`, `HookCanEmit`, ...) for `rshooks` to extract into a sidecar JSON. Build-only — adds nothing to the final wasm. | `metadata! { name: "accept-all", HookOn: [Invoke], HookName: "accept" }` |

See [Hook Metadata](../build/metadata.md).
