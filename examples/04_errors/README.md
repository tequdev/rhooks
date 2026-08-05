# errors

## What you'll learn

How to give a hook its own **meaningful, stable rollback error-code
system** — instead of every failure path calling `rollback!(msg, -1)` with
the same undifferentiated code, each rejection reason gets its own code and
message, so anything inspecting the transaction afterwards (an indexer, a
wallet, a support script) can tell *why* it was rejected without parsing the
message text.

## Where the code ends up: `HookReturnCode`

The second argument to `rollback(msg, code)` (and `accept(msg, code)`) is
not discarded — xahaud records it in the transaction's metadata as
`HookExecution.HookReturnCode` (a decimal integer over RPC/WS, per the
`hook-e2e-testing` skill). This is this hook's own application-defined
result code, distinct from (and not to be confused with) the Hook API's
internal `-1..=-45`/`-10024` error codes returned by individual host calls
like `otxn_field` (see `rshooks::error::HookError`) — those are about
*why a host call failed*, not about *why the hook rejected the
transaction*. This example's codes are chosen well outside that range
(`-101..=-104`) specifically so they're unambiguous at a glance.

## Code walkthrough

`RejectReason` is defined with [`rshooks::hook_errors!`] — one variant
per rejection reason, each given its own explicit negative discriminant —
plus a small hand-written `impl` for the parts the macro doesn't generate
(a message per variant, and a `rollback` convenience):

```rust
hook_errors! {
    pub enum RejectReason {
        BadAccountField = -101,
        BlockedSourceTag = -102,
        NotNativeAmount = -103,
        AmountTooLarge = -104,
    }
}

impl RejectReason {
    fn message(self) -> &'static [u8] { /* one fixed string per variant */ }
    fn rollback(self) -> ! { rollback!(self.message(), self) }
}
```

`hook_errors!` expands the enum declaration into a `#[repr(i64)]` enum plus
`impl From<RejectReason> for i64` and an inherent `fn code(self) -> i64` —
this is the same macro `firewall`, `state-counter`, and `emit-txn` use for
their own rollback codes (see each one's README); `rollback!`'s `code`
argument accepts any `Into<i64>` value, so `rollback!(msg, self)` above
converts `self` through that `From` impl directly, without calling `.code()`
by hand.

`hook()` runs a short chain of checks — sender readable, `SourceTag` not
blocklisted, `Amount` is native and within a policy limit — calling
`RejectReason::rollback()` the first time one fails. Because `rollback`
(and hence `RejectReason::rollback`) returns `!` (the never type), each
`match`/`if` arm that calls it type-checks against whatever the other arms
return, with no placeholder value needed.

`SourceTag` is an *optional* transaction field: `otxn_field_u64` returning
`Err(HookError::DoesntExist)` means "no tag was set," which is not itself a
policy violation, so only an exact match on the blocked value triggers a
rollback — every other `Result` (`Ok` with a different tag, or any `Err`)
falls through to the `_ => {}` arm and continues.

## HookReturnCode table

| Code | Reason | Message |
|-----:|--------|---------|
| `0` | (via `accept!()`) | every check passed |
| `-101` | `BadAccountField` | `errors: could not read otxn Account` |
| `-102` | `BlockedSourceTag` | `errors: blocked SourceTag` |
| `-103` | `NotNativeAmount` | `errors: unsupported (non-native) Amount` |
| `-104` | `AmountTooLarge` | `errors: amount exceeds policy limit` |

(Compare with `rshooks::error::HookError`'s codes, `-1..=-45` plus
`-10024` — a *Hook API* failure surfaces as one of those instead, from
whichever host call failed, before this hook ever gets to choose one of its
own codes.)

## Build

```sh
cargo run -p rshooks-build -- build --manifest-path examples/04_errors/Cargo.toml
```

No extra flags needed: `SourceTag`/`Amount` comparisons here are all
between plain integers (`u64`), never fixed-size arrays, so there's no
compiler-generated comparison loop to guard (contrast with `firewall`).

## Expected behavior

- Sender unreadable → rollback, code `-101`.
- `SourceTag == 13` → rollback, code `-102`.
- `Amount` not native (an IOU) → rollback, code `-103`.
- Native `Amount` over 100 XAH → rollback, code `-104`.
- Otherwise → accept, code `0`.
