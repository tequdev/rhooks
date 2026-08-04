# reward

A behavior-equivalent Rust port of xahaud's genesis `RewardHook`:
[`hook/genesis/reward.c`](https://raw.githubusercontent.com/Xahau/xahaud/dev/hook/genesis/reward.c).

`ttCLAIM_REWARD` (98) transactions land on an account that has this hook
installed. The hook computes the account's accrued reward since its last
claim, converts it through the governance-set reward rate (`RR`/`RD`
hook state, normally written by [`../81_govern`](../81_govern)), and
emits a `GenesisMint` transaction crediting the claimant and every
active-validator L1 governance seat.

Build: `rshooks-build build --manifest-path examples/80_reward/Cargo.toml`
(also wired into `mise run build-examples`).

- Worst-case instructions: **13680** (`hook`), 0 (`cbak` — none declared)
- Max block/loop/if nesting: **24** (limit: 32)
- Binary size: **7175 bytes**

## Files

| File | Contents |
|---|---|
| `src/lib.rs` | Hook entry point: otxn/config checks, reward-rate math, orchestrates `mint_txn` and the L1-seat reward loop |
| `src/mint_txn.rs` | Byte-exact `GenesisMint` transaction builder |
| `src/message.rs` | The `ClaimReward`-too-early rollback message (digit-patched, matching reward.c's `msg_buf`) |
| `src/raw.rs` | `float_*` raw Hook API wrappers, scoped to the XFL reward-rate math — see "Toolchain limitation" below |

## Hook state

| Key | Value | Notes |
|---|---|---|
| `"RR"` (2 bytes) | 8-byte LE XFL | Reward rate, written by govern.rs/govern.c. Falls back to reward.c's compiled-in default (`0.00333333333`) if absent. |
| `"RD"` (2 bytes) | 8-byte LE XFL | Reward delay (seconds), written by govern.rs/govern.c. Falls back to reward.c's compiled-in default (2,600,000s = ~30 days). |
| `{seat byte}` (1 byte) | 20-byte AccountId | Reverse seat -> member lookup, written by govern.rs/govern.c — used to find which L1 members are also active validators. |

No other state is read or written by this hook.

## Hook parameters

None. `reward` reads its configuration entirely from hook state (`RR`/`RD`,
governed by `81_govern`), matching reward.c exactly.

## Behavior-equivalence table

Each row is an input case and the branch of reward.c it corresponds to
(line numbers are reward.c's, current `dev` branch as of this port).
"Same input -> same output" is the equivalence target: state read/write
key+value layout, `accept`/`rollback` outcome, and the emitted
transaction's bytes. `rollback`/`accept` **codes** are *not* part of the
target — see "Differences from reward.c" below.

| # | Input case | reward.c | This crate | Outcome |
|---|---|---|---|---|
| 1 | `otxn_type() != ttCLAIM_REWARD` | L106 | `my_hook`, first check | `accept("Reward: Passing non-claim txn")` |
| 2 | sender == hook account (hook's own outgoing txn) | L117 | `buf_eq_20` check | `accept("Reward: Passing outgoing txn")` |
| 3 | `RR`/`RD` state missing | L125-126 | `state_xfl(...).unwrap_or(default)` | Falls back to compiled-in defaults, continues |
| 4 | `RR <= 0` or `RD <= 0` | L129 | `rewards_disabled` check | `rollback("Reward: Rewards are disabled by governance.")` |
| 5 | `RD` truncates to a negative second count, or `RR` is negative/`> 1`, or `RD < 1` | L134-137 | `misconfigured` check | `rollback("Reward: Rewards incorrectly configured by governance or unrecoverable error.")` |
| 6 | Sender's AccountRoot has no `RewardAccumulator` (first-ever claim) | L147-148 | `slot_subfield(..., sfRewardAccumulator, ...)` check | `accept("Reward: Passing reward setup txn")` |
| 7 | `time_elapsed < required_delay` | L161-174 | Same check | `rollback("You must wait NNNNNNN seconds")` — digits patched from the remaining delay |
| 8 | `RewardLgrFirst <= 0` or `RewardLgrLast <= 0` | L188 (`ASSERT`) | `(first <= 0) \| (last <= 0)` | `rollback("Reward: Assertion failure.")` |
| 9 | `ledger_seq() - RewardLgrFirst <= 0` | L196 (`ASSERT`) | `elapsed <= 0` check | `rollback("Reward: Assertion failure.")` |
| 10 | Valid claim, matured | L200-230 | reward-rate math (`raw::float_*`) | Builds and emits a `GenesisMint`: rewardee entry (accrued reward + otxn fee refund) plus one entry per L1 seat held by an active validator |
| 11 | `UNLReport` object absent/unreadable | L269-311 | `push_l1_seat_entries` early-returns | Rewardee-only `GenesisMint` (no L1 seat entries) — matches reward.c's `if (slot_set(...) == 1 && ...)` guard around the whole L1 distribution |
| 12 | A validator's owning account isn't a current governance seat holder | L288 | `can_reward` lookup miss | That validator's seat gets no entry |
| 13 | `emit()` fails | L343-344 | `emit_buf` check | `rollback("Reward: Emit loopback failed.")` |
| 14 | Emit succeeds | L347 | — | `accept("Reward: Emitted reward txn successfully.")` |

## `GenesisMint` wire format

Built field-by-field in `mint_txn.rs` from
[`rshooks::txn::codec`](../../crates/rshooks/src/txn.rs)'s generic
STObject primitives (the same ones `txn_template!` uses), not a
hand-transcribed byte table — but the resulting bytes are byte-for-byte
what reward.c's own `txn_mint`/`template` arrays produce:

`TransactionType(96) | Flags(tfCANONICAL) | Sequence(0) |
FirstLedgerSequence | LastLedgerSequence | Fee | SigningPubKey (35-byte
zero blob) | Account (genesis) | EmitDetails | GenesisMints[]`

Each `GenesisMints` entry: `GenesisMint { Amount, Destination }` — 34
bytes (`2 + 9 + 22 + 1`). Up to 21 entries (rewardee + 20 L1 seats).

`txn_template!` isn't used directly because it only supports a fixed
field list; `GenesisMints` is a variable-length array.

## Differences from reward.c

| # | reward.c | This crate | Why |
|---|---|---|---|
| 1 | `rollback`/`accept` codes are `__LINE__` (the C source's line number) | Codes are a small `hook_errors!` enum (`RewardError`, see `lib.rs`) or `0` | reward.c's line-number codes aren't meaningful protocol data (only debugging aid for the C source); this repo's convention is a stable enum. `ter` (`tesSUCCESS`/`tecHOOK_REJECTED`) and the rollback/accept **message** are unaffected and match exactly. |
| 2 | `seat > L1SEATS` (not `>=`) lets a stored seat byte of exactly 20 through, then writes `can_reward[20]` — out of bounds on a 20-element C array (UB) | Same `> L1_SEATS` condition preserved for state-layout parity, but `can_reward.get_mut(seat)` turns the out-of-range write into a safe no-op | `seat` values only ever come from govern.rs/govern.c, which never assigns seat 20 (`SEAT_COUNT = 20`, loop bound `i < member_count <= 20`), so this is unreachable in practice either way — this crate just doesn't reproduce the UB if it somehow were reached. |
| 3 | reward.c's `float_*` reward-rate math treats every host-call result as a raw, unchecked `i64` | `src/raw.rs` calls `float_set`/`float_divide`/`float_multiply`/`float_int`/`float_sign`/`float_one`/`float_compare` through `rshooks_core`'s raw FFI directly, not `rshooks::xfl::XFL`'s validated `Result<XFL, HookError>` API | Not a build constraint — a deliberate semantic match: reward.c folds host failure into the *same* validity checks it already needs for legitimately out-of-range values, never asking "did this call fail" on its own. See `src/raw.rs`'s module doc comment. Every other Hook API call in this crate uses `rshooks::api`'s ordinary wrappers — see "Toolchain limitation" below. |

No other intentional behavioral differences. In particular: the reward
formula, the delay/rate validity bounds, the fee-refund addition, the
`Balance`-field masking (`& ~0xE000000000000000`), and the `UNLReport`
seat-lookup loop bounds (`MAXUNL = 128`, `L1SEATS = 20`) are all
transcribed exactly.

## Toolchain limitation: `HookError` decoding and nesting depth

`rshooks-build`'s Guard-type pipeline inlines every function in a crate
into `hook()`/`cbak()` (`docs/DESIGN.md` §6.2c), then runs a
ladder-flattening pass (`unnest.rs`) to keep the merged function's
block/loop/if nesting under the vendored guard checker's 32-level limit.
That pass only collapses a "diverging tail" — instructions that push
constants, call an *imported* function, and end in `unreachable` — which
matches a plain `rollback!(literal_message, literal_code)` call exactly.

`rshooks::error::res` (the function every `rshooks::api::*`/`XFL`
wrapper funnels its host-call result through) converts a negative raw
return code to a concrete `HookError` value via a ~40-arm `match` —
`HookError::from(i64)`. Measured directly (via
`crates/rshooks-build/examples/diag.rs`, a throwaway pipeline-stage dumper
written during this investigation — dumps the `clean`/`flatten`/`unnest`
intermediate `.wasm` at each stage so `wasm-tools print` can inspect real
nesting depth and find the exact construct responsible), this compiles
to a wasm `br_table` needing ~40 nested `block`s. It shows up, though,
**only** at a call site that actually inspects which specific
`HookError` variant a failure was — `match ... { Err(HookError::Xxx) =>
..., ... }` — because the compiler then has to prove which of the ~40
raw codes it is. A call site that only asks "did this fail"
(`.is_err()`, `.unwrap_or(default)`, comparing the whole `Result` against
one specific `Ok` value) never reads which variant it got, so the
optimizer discards the decode entirely and keeps just the "is the raw
code negative" branch. This crate's every Hook API call (besides the XFL
math below) is written the second way, so `rshooks::api`'s ordinary
`Result`-based wrappers (including `_exact`/`_buf` convenience variants
where they fit) are used directly throughout `lib.rs` and
`mint_txn.rs` — no `raw` module needed for any of it. (`../81_govern`
has exactly one call site where the specific-variant form was actually
wanted — see its README's copy of this section for why, and how it was
avoided there too.)

Getting comfortably under the 32-level limit (measured: max nesting
depth 24) took the structural techniques below, independent of which API
wraps each call:

- bumping `examples/Cargo.toml`'s shared `[profile.release]` `opt-level`
  from `"z"` to `3` for this crate specifically (helps LLVM eliminate
  more dead code, including the unused-variant decode above);
- restructuring boolean chains to use eager `|`/`&` instead of
  short-circuiting `||`/`&&` feeding `.unwrap_or(default)`;
- converting `MintTxn`'s internal API from `Result`-propagating (`?`) to
  rollback-directly-on-failure (see `mint_txn.rs`'s module doc comment —
  this by itself fixes the analogous "return ladder" problem for
  ordinary, non-panicking early returns);
- replacing `codec::field_header(sfXxx)` runtime calls (whose internal
  range-check `assert!`s are only documented as safe to elide in a
  `const` context) with precomputed `const` headers (`HDR_*` in
  `mint_txn.rs`);
- replacing `<[u8]>::copy_from_slice` (which panics — and needs to keep
  its length-mismatch-message formatting machinery linked in — whenever
  the compiler can't prove both slices are the same length, which it
  generally can't across a `get_mut(range)` boundary) with
  `Iterator::zip`, which has no such panicking case;
- marking the small, extremely hot helper functions (`fail`, `push`,
  `push_field_header`, `push_u32_field`, `write_native_amount`)
  `#[inline(always)]`, so their own diverging checks get exposed at
  every call site for `unnest` to collapse (this was the single largest
  win: **42 -> 23** in one step) — apparently `rshooks-build`'s later
  wasm-level `flatten` pass merges function *bodies* mechanically but
  does not itself re-run the dead-code elimination LLVM would have done
  had the functions been Rust-level-inlined first;
- writing seat-iteration as a manual `while` loop instead of `for seat in
  0u8..N`: a `for` loop's `Iterator::next()` bookkeeping compiles to real
  instructions *before* the loop body, which pushes `guard!`'s `_g` call
  out of the required first-three-instructions position the checker
  looks for.

`src/raw.rs` closes the one remaining, deliberately narrow gap: every
`float_*` call reward.c's reward-rate math makes is called
through a thin `unsafe` wrapper around `rshooks_core`'s raw extern
declaration instead of `rshooks::xfl::XFL`. This is **not** primarily a
nesting-depth workaround — see `src/raw.rs`'s module doc comment for the
XFL-validation-semantics rationale, which is the actual reason it exists.

The unavoidable-per-call-site `HookError` decode cost above is still
flagged as a **candidate `rshooks`/`rshooks-build` fix** (e.g. `flatten.rs`
re-running a cheap DCE pass after merging, so a Rust-level-inlined
non-generic `res()` doesn't need every call site to independently prove
its result unused) for the rarer case where a hook genuinely needs to
distinguish specific `HookError` variants at several call sites at once —
`examples/07_xfl-math` and the other smaller examples in this repo don't
hit it simply because they don't have that many such call sites in one
function.

## Testing

See `e2e/test/reward.test.ts` and the top-level PR description for the
live-node test matrix (against `XahauGenesis_test.cpp`'s `ClaimReward`
coverage) and which cases could/couldn't be reproduced outside a real
genesis-activated network.

## Slot API: the typed layer, with an extraction

The account-root reads go through `rshooks::slot_obj`: `SlotObject::from_keylet`
loads the sender's account, `.get(sfRewardAccumulator)` and friends derive the
field handles, and `.value()` reads them. No slot numbers appear.

Two details are worth knowing before touching `read_reward_fields`:

- **It is `#[inline(never)]`, and that is load-bearing.** This hook sits at
  nesting depth 26 of the Hook API's 32-level limit. Five `Result`-returning
  typed reads inlined into `my_hook` measured **68** — the build is rejected
  outright. In their own frame they cost nesting the entry point never sees.
  Splitting the four presence checks into sequential `let else`s rather than
  one 4-way tuple pattern was the other half of the fix: a tuple
  `let (Ok(..), Ok(..), Ok(..), Ok(..)) = .. else` lowers to nested matches.
- **`sfBalance` is read with `assume_type::<u64>()`.** The field is an
  `Amount`, but what the reward math wants is the native amount's raw 64-bit
  wire encoding — it masks the flag bits off itself — not a classified
  `AmountBytes`. `assume_type` is the documented escape for "I know what is
  in this slot"; the `u64` read decodes the same eight big-endian bytes the
  previous `slot_u64` call did. `sfRewardLgrFirst`/`sfRewardLgrLast`/
  `sfRewardTime` are UInt32 fields, so their typed reads hand back `u32`
  where `slot_u64` handed back a widened `u64`; the widening is now explicit
  at the read site.

**Measured cost of the migration: +220 worst-case instructions (13766 →
13986) and +434 bytes.** Most of it is the 34-byte keylet copy
`SlotObject::from_keylet(&Keylet(*kl_buf))` makes where the raw `slot_set`
took a slice, plus the `Result` plumbing on five reads that were previously
`== Ok(n)` integer comparisons. The hook stays well inside both ceilings.

`src/raw.rs` shrank by exactly one function: `slot_float` was a raw *slot*
API and went with the rest. Its one caller — the otxn fee read — now goes
through `fee_slot.as_xfl()`, folding a failed call into the same `> 0` test
the value needed anyway, which is what reward.c's unchecked `int64_t` did by
other means. The remaining `float_*` shims are untouched: they exist for the
raw-`i64` semantics described in the table above, and none of that is slot
navigation.
