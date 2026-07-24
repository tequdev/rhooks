# state-foreign

## What you'll learn

How to read **another account's** Hook state with `state_foreign` — the
same hook installed on multiple accounts can use this to read a peer's
configuration, or one hook can gate its own behavior on a flag maintained
by a separate "registry"/"oracle" account.

## Code walkthrough

```rust
match state_foreign(&mut flag, &ENABLED_KEY, None, Some(&target)) {
    Ok(n) if n == flag.len() => {}
    Err(HookError::DoesntExist) => rollback!(b"...", -1),
    _ => rollback!(b"...", -1),
}
```

`state_foreign(out, key, namespace, account)` takes two `Option<&[u8]>`
parameters beyond the plain `state` call: `namespace` and `account`, both
defaulting to "this hook's own" when `None` (see
`hooks_lib::api::state::state_foreign`'s doc comment). Passing
`namespace = None` and `account = Some(&target)` reads the entry keyed
`ENABLED_KEY` **in this hook's own namespace, but on `target`'s account** —
the natural shape for "the same hook code, installed on account A and
account B, where A wants to read a flag B's copy of the hook maintains
about itself." Reading a genuinely different hook's namespace on a foreign
account would need an actual `namespace` value too (out of scope for this
minimal example).

`target` itself comes from a Hook parameter (`ACCT`), following the same
"config via `hook_param`" idiom as `examples/hook-params` — see that
example's README for the hex-encoding/`SetHook` details, which apply here
unchanged (just a different parameter name and a 20-byte `AccountId`
payload instead of an 8-byte integer).

`state_foreign`'s `Err(HookError::DoesntExist)` (no entry at all — the
common, expected "not configured" case) is deliberately distinguished from
every other `Err` (e.g. a malformed `target` or an unexpected host
failure), each producing its own rollback message, following the same
"give each failure a distinct outcome" idea as `examples/errors` (though
without a full custom-code table here — just a message per case).

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/state-foreign/Cargo.toml
```

No extra flags needed: every comparison here is a scalar (`usize`/`u8`)
comparison, not a fixed-size array comparison, so there's no
compiler-generated `bcmp`-style loop to guard.

## Configuring the target account and its flag

Set an `ACCT` Hook parameter (20 raw bytes, the account to read from) when
installing this Hook, the same way `hook-params`' README shows for `MIN`
(just a different parameter name and payload shape — 20 raw bytes instead
of 8). On the **target** account, this hook's namespace must have a
32-byte state entry keyed by `pad!(b"enabled")` (`"enabled"` followed by
zero bytes to 32 bytes total) whose first byte is nonzero — e.g. set with
`state-counter`'s `state_set` pattern, or any tooling that can write raw
hook state. Deployment/state-seeding tooling itself is out of scope for
this repo (see `docs/DESIGN.md` §1 non-goals).

## Expected behavior

- `ACCT` not configured (or not 20 bytes) → rollback.
- `ACCT` configured, but the target account has no `enabled` entry in this
  hook's namespace → rollback (`"not configured on target account"`).
- `ACCT` configured, `enabled` entry present but its first byte is `0` →
  rollback (`"target account's flag is off"`).
- `ACCT` configured, `enabled` entry present with a nonzero first byte →
  accept.
