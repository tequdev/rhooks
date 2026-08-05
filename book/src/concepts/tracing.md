# Tracing and Debugging

Hooks have no debugger and no stdout. The Hook API's `trace*` family of
host calls is the only window into what a hook is doing while it runs, and
`rshooks` wraps them in three macros — `trace!`, `trace_num!`, and
`trace_float!` — along with two small compile-time helper macros, `pad!`
and `pad_left!`, that are unrelated to tracing but live alongside these
macros in `rshooks`'s `macros` module. This page covers all five, and where
tracing output actually goes.

## `trace!`, `trace_num!`, `trace_float!`

```rust,ignore
trace!(msg);              // message only
trace!(msg, data);        // message + a byte slice
trace_num!(msg, number);  // message + an i64
trace_float!(msg, value); // message + an XFL value
```

`msg` is a raw byte slice, as with `accept!`/`rollback!` — there's no
`format!` available in a `no_std` hook, so build any dynamic content as
bytes ahead of time rather than reaching for string formatting. `trace!`'s
two-argument form's `data` is rendered as raw bytes by the underlying host
call, not hex — see the hex-dump note below for the raw-layer alternative
if you need that.

Underneath the macros, `rshooks::api::trace` exposes the same three
operations as plain functions you can call directly regardless of feature
flags:

```rust,ignore
pub fn trace(msg: &[u8], data: &[u8], as_hex: bool) -> Result<i64>;
pub fn trace_num(msg: &[u8], number: i64) -> Result<i64>;
pub fn trace_float(msg: &[u8], value: XFL) -> Result<i64>;
```

`trace`'s `as_hex` parameter is the hex-dump switch the macros don't
expose: passed as `true`, `data` is rendered as a hex dump by the host
instead of raw bytes — useful for inspecting something like a raw
`AccountId` or transaction hash byte-for-byte. Reach for
`rshooks::api::trace::trace(msg, data, true)` directly when you need that;
the `trace!` macro's `(msg, data)` form always passes `as_hex: false`.

## Where output goes

Trace output is not visible in a transaction's result or metadata — it
goes to the Hook host's own debug/trace log, visible when the node it runs
on is started with trace logging enabled. This makes tracing purely a
development and debugging tool: useful while writing and testing a hook
against a local or test node, but not something a production hook's
correctness should ever depend on, and not something an external observer
(an indexer, a wallet) can see. If a value needs to be visible to the
outside world, it belongs in the `accept!`/`rollback!` message or code
(see [Accept, Rollback, and Errors](errors.md)), not a trace call.

## Tracing costs bytes and instructions

Every trace call — like every other host call — costs execution
instructions and adds to a hook's worst-case instruction count, and the
`msg`/`data` bytes it's given are real bytes the hook has to construct.
That's exactly why the `trace!`/`trace_num!`/`trace_float!` macros are
feature-gated: they compile to nothing at all unless `rshooks`'s own
`trace` feature is enabled.

```toml
rshooks = { version = "...", features = ["trace"] }
```

No feature re-declaration is needed in the hook crate itself beyond
enabling it on the `rshooks` dependency — the macros expand to calls into
`rshooks::api::trace::__macro_support`'s shim functions, which are always
present but only forward to the real host call when the feature is on;
otherwise they're a no-op that still consumes (and thus doesn't warn about)
their arguments. This lets a hook crate leave `trace!`/`trace_num!`/
`trace_float!` calls in its source permanently, toggling them on only for
a debug build, without editing the call sites at all. The plain functions
in `rshooks::api::trace` (`trace`, `trace_num`, `trace_float`) are
unconditional — call those directly instead if you want tracing regardless
of feature flags.

## `pad!` and `pad_left!`

These two macros are unrelated to tracing, but worth knowing alongside it
since real hook code often builds byte buffers to hand to `trace!` (or
`accept!`/`rollback!`) by hand. Both zero-pad a constant byte string to a
fixed-size array, entirely at compile time — no runtime copy or zeroing
loop, so no guard is needed for either:

```rust
use rshooks::pad;

let padded: [u8; 10] = pad!(b"hello");
assert_eq!(padded, [b'h', b'e', b'l', b'l', b'o', 0, 0, 0, 0, 0]);
```

`pad!` right-pads (`src` at the front, zero bytes at the end); `pad_left!`
is its mirror image, left-padding instead:

```rust
use rshooks::pad_left;

let padded: [u8; 10] = pad_left!(b"hello");
assert_eq!(padded, [0, 0, 0, 0, 0, b'h', b'e', b'l', b'l', b'o']);
```

Both expand to an inline `const` block, so the source must be a constant
expression, and a source longer than the destination is a **compile
error**, never a runtime panic — the array length itself is inferred from
the surrounding context (a `let` binding's type, a struct field, and so
on).

It's worth noting what these are *not* for anymore: building a short hook-
state key. A `[u8; N]` (with `1 <= N <= STATE_KEY_LEN`) works directly as a
`StateKeyEncode` key at its own real length — the host itself left-pads a
short key internally, so a Rust hook doesn't need to reproduce that padding
locally (see [Hook State](../data/state.md)). Reach for `pad!`/`pad_left!`
when a fixed-size buffer genuinely needs local padding for some other
reason — building an already-full-width constant on purpose, or padding a
byte string for a use unrelated to hook-state keys.

## Where to go next

- [Accept, Rollback, and Errors](errors.md) covers `accept!`/`rollback!`,
  the macros that share `trace!`'s raw-byte-slice message grammar.
- [Guards and Loops](guards.md) covers the other place a hook's
  instruction budget matters.
- [Macro Reference](../reference/macros.md) is the full grammar listing for
  every macro on this page.
