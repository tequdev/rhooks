# hook-params

## What you'll learn

How to make a Hook's behavior configurable at install time via a **Hook
parameter** (`hook_param`), with a sensible compiled-in default when the
operator doesn't set one.

## The hook

Rolls back the originating transaction if its native (XRP/XAH) `Amount` is
below a minimum threshold; accepts otherwise. The threshold comes from a
Hook parameter named `MIN` — 8 raw bytes, a big-endian `u64` drops value —
falling back to a baked-in default (`1,000,000` drops = 1 XAH) if `MIN`
isn't configured.

## Code walkthrough

```rust
fn min_drops() -> u64 {
    let mut raw = [0u8; 8];
    match hook_param(&mut raw, MIN_PARAM) {
        Ok(n) if n == raw.len() => u64::from_be_bytes(raw),
        _ => DEFAULT_MIN_DROPS,
    }
}
```

`hook_param` is a caller-buffer API: it writes into `raw` and returns the
number of bytes written (or `Err(HookError::DoesntExist)` if the parameter
isn't set at all). Checking `n == raw.len()` handles both "not configured"
and "configured with a value of the wrong size" the same way — fall back to
the default — without treating a malformed parameter as a hard error. This
mirrors `firewall`'s `hook_param` pattern for its `BL` parameter, but reads
a threshold instead of an `AccountId`.

The originating transaction's `Amount` is read the same way, via
`otxn_field(&mut amount_raw, sfAmount)`, and only accepted if it comes back
as exactly 8 bytes (a native amount serializes as 8 bytes; an IOU amount is
48). The top two bits of a serialized native amount are format flags, not
part of the drops value (`0x80` = "not an IOU", `0xC0`'s low bit = sign,
always set since XRP/XAH amounts are never negative) — see
`hooks_lib::txn::codec::encode_native_amount_const`'s doc comment for the
same bit layout used in the other direction (encoding a drops value for an
emitted transaction). Masking `NATIVE_AMOUNT_FLAG_BITS` off recovers the
plain drops magnitude.

This example intentionally only supports native amounts — reading *any*
`Amount` kind (native or IOU) uniformly is what `examples/xfl-math` is for.

## Hook parameter hex encoding

`MIN` must be exactly 8 bytes, big-endian. For a threshold of `5,000,000`
drops (5 XAH):

```
decimal:  5000000
hex (u64, big-endian): 00 00 00 00 00 4C 4B 40
```

In a `SetHook` transaction's `HookParameters` array, this becomes one
`HookParameter` entry:

```json
{
  "HookParameter": {
    "HookParameterName": "4D494E",
    "HookParameterValue": "00000000004C4B40"
  }
}
```

`HookParameterName` is the hex encoding of the ASCII parameter name (`MIN`
→ `4D494E`); `HookParameterValue` is the hex encoding of the 8 raw bytes
above. Omitting the `MIN` entry entirely falls back to the compiled-in
1 XAH default.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/hook-params/Cargo.toml
```

No extra flags needed: every comparison here is between plain integers
(`u64`), not fixed-size arrays, so there's no compiler-generated
`bcmp`-style loop to worry about (contrast with `firewall`, which compares
two `[u8; 20]`s and needs `--auto-guard`).

## Expected behavior

- `MIN` unset, `Amount` = 1 XAH or more → accept.
- `MIN` unset, `Amount` below 1 XAH → rollback (`"hook-params: amount below
  configured minimum"`).
- `MIN` set to some threshold, `Amount` at or above it → accept.
- `MIN` set, `Amount` below it → rollback.
- `Amount` is an IOU (not native XRP/XAH) → rollback (`"hook-params:
  unsupported (non-native) Amount"`), regardless of `MIN`.
