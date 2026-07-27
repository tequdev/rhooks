# govern

A behavior-equivalent Rust port of xahaud's genesis `GovernanceHook`:
[`hook/genesis/govern.c`](https://raw.githubusercontent.com/Xahau/xahaud/dev/hook/genesis/govern.c).

A 20-seat round-table governance hook. Installed on the genesis account it
is the **L1 table**; installed on any other (blackholed) account it is an
**L2 table**. Members vote on topics (seats, hooks, reward rate/delay
[L1 only]); once a topic's votes cross a threshold, the vote is
"actioned" — applied directly (seat/hook/reward change) or, for an L2
table voting on an L1 topic, forwarded to L1 as an `Invoke`.

Build: `hooks-build build --manifest-path examples/81_govern/Cargo.toml`
(also wired into `mise run build-examples`).

- Worst-case instructions: **41510** (`hook`), 0 (`cbak` — none declared)
- Max block/loop/if nesting: **22** (limit: 32)
- Binary size: **13757 bytes**

## Files

| File | Contents |
|---|---|
| `src/lib.rs` | Hook entry point, setup, vote processing/threshold logic, the three action handlers (`action_reward`/`action_hook`/`action_seat`), vote garbage collection |
| `src/keys.rs` | State-key layouts (`vote_key`, `vote_count_key`, seat/member keys) |
| `src/txn.rs` | Byte-exact encoders for the two transactions this hook emits (L1-vote-forward `Invoke`, `HookSet`) |
| `src/raw.rs` | Thin raw Hook API wrappers — see "Toolchain limitation" below |

## Hook state

| Key | Value | Notes |
|---|---|---|
| `"MC"` (2 bytes) | 1-byte member count | |
| `"RR"` (2 bytes) | 8-byte LE XFL | Reward rate — L1 table only, read by `../80_reward`. |
| `"RD"` (2 bytes) | 8-byte LE XFL | Reward delay — L1 table only, read by `../80_reward`. |
| `{seat}` (1 byte, `0..=19`) | 20-byte AccountId | Forward: seat -> member. |
| `{20-byte AccountId}` | 1-byte seat | Reverse: member -> seat. |
| `{'V', topic_type, topic_id, layer, 0×8, 20-byte voter}` (32 bytes) | The voter's vote data for that topic (right-aligned, front-padded with zeros for < 32-byte topics) | One entry per (topic, voter). |
| `{'C', topic_type, topic_id, layer, <front-truncated topic data>}` (32 bytes) | 1-byte vote count | One entry per (topic, topic **data**) combination actually voted for. See "Differences" below for the 32-byte (`'H'`) topic collision this key shape inherits from govern.c. |

`topic_type` is `'S'`/`'H'`/`'R'`; `topic_id` is the seat number (`'S'`),
hook slot (`'H'`), or `'R'`/`'D'` (`'R'`); `layer` is `1` or `2` (always
`1` on an L1 table).

## Hook parameters

| Name | Meaning | Used when |
|---|---|---|
| `IMC` (3 bytes: `'I','M','C'`) | Initial member count (1-byte value) | First `Invoke` (setup) |
| `IS{seat}` (3 bytes: `'I','S',seat`) | Seat `seat`'s initial member (20-byte AccountId value) | Setup, once per seat `0..IMC` |
| `IRR` (3 bytes: `'I','R','R'`) | Initial reward rate (8-byte LE XFL value) | Setup, L1 table only |
| `IRD` (3 bytes: `'I','R','D'`) | Initial reward delay (8-byte LE XFL value) | Setup, L1 table only |

## `Invoke` parameters (otxn, not hook params)

| Name | Meaning |
|---|---|
| `T` (1 byte) | 2-byte value: `{topic_type, topic_id}` |
| `V` (1 byte) | The vote's data: 8 bytes (`'R'`), 20 bytes (`'S'`), or 32 bytes (`'H'`) |
| `L` (1 byte) | 1-byte value: `1` or `2` — which layer this vote targets (L2 tables only; an L1 table's own votes are always layer 1) |

## Behavior-equivalence table

Each row is an input case and the govern.c branch it corresponds to
(line numbers are govern.c's, current `dev` branch as of this port).

| # | Input case | govern.c | Outcome |
|---|---|---|---|
| 1 | `otxn_type() != ttINVOKE` | L128 | `accept("Governance: Passing non-Invoke txn...")` |
| 2 | Self-triggered, `Destination != hook account` | L138-144 | `accept("Goverance: Passing outgoing txn.")` |
| 3 | `"MC"` state missing (first `Invoke` ever) | L156 | Setup path: reads `IMC`(+`IRR`/`IRD` on L1)/`IS*` params, populates seat table, `accept("...Setup completed successfully.")` |
| 4 | Sender has no seat (`{account}` reverse key missing) | L225-227 | `rollback("...You are not currently a governance member...")` |
| 5 | Missing/invalid `T` otxn parameter | L234-242 | `rollback("...Valid TOPIC must be specified...")` |
| 6 | `T`'s seat/hook/reward id out of range | L245-252 | `rollback` with the matching range message |
| 7 | L2 table, missing/invalid `L` parameter | L256-266 | `rollback` with the matching message |
| 8 | `L == 2` and topic is `'R'` | L268-269 | `rollback("...L2s cannot vote on RR/RD at L2...")` |
| 9 | Missing/wrong-size `V` parameter | L281-283 | `rollback("...Missing or incorrect size of VOTE data...")` |
| 10 | Vote identical to the voter's existing vote for this topic | L314-315 | `accept("...Your vote is already cast this way...")` |
| 11 | New vote recorded, threshold not yet met | L381-393 | `accept("...Not yet enough votes to action...")` (message varies: L1/L2-topic-at-L2 vs L2-table-voting-on-L1-topic) |
| 12 | Threshold met, L2 table voting on an L1 topic (`l == 1`) | L401-472 | Emits an `Invoke` to L1 carrying `T`/`V`; `accept`/`rollback` on emit success/failure |
| 13 | Threshold met, topic `'R'` | L477-487 | Writes `"RR"`/`"RD"`; `accept` with the matching message |
| 14 | Threshold met, topic `'H'`, same hook already installed | L505-514 | `accept("Goverance: Target hook is already the same...")` |
| 15 | Threshold met, topic `'H'`, hash doesn't exist on ledger (non-delete) | L517-524 | `rollback("...Hook Hash doesn't exist on ledger...")` |
| 16 | Threshold met, topic `'H'` | L527-554 | Emits a `HookSet` installing/deleting hook slot `n`; `accept`/`rollback` on emit success/failure |
| 17 | Threshold met, topic `'S'`, seat already holds the voted member | L568-569 | `accept("...seat already contains the new member.")` |
| 18 | Threshold met, topic `'S'`, member count would drop to 1 | L615 (`ASSERT`) | `rollback("Governance: Assertion failed.")` |
| 19 | Threshold met, topic `'S'` | L557-710 | Full add/move/delete logic (see the 8-case `E\|Z\|M` table in govern.c's own comments), including vote garbage collection for an outgoing seat holder; `accept("Governance: Action member change.")` |

## Differences from govern.c

| # | govern.c | This crate | Why |
|---|---|---|---|
| 1 | `rollback`/`accept` codes are `__LINE__` | Codes are a small `hook_errors!` enum (`GovernError`) or `0` | Same rationale as `../80_reward`'s `RewardError` — see its README. |
| 2 | The "D" debug-line `Invoke` parameter (`trace_num` of a test-supplied line number) | Not implemented | Purely a debug aid for govern.c's own C++ test harness (`XahauGenesis_test.cpp`'s `DBGLN` trace); never observable in `accept`/`rollback`/state/emitted-txn output. |
| 3 | `q80`/`q51` computed via `member_count * 0.8`/`* 0.51` (hardware `double` multiplication, truncated) | Computed via exact integer arithmetic (`* 4 / 5`, `* 51 / 100`) | The Hook API's guard checker **rejects wasm floating-point opcodes outright** for a Guard-type hook — `f64.mul` et al. are not in the allowed instruction set. For every `member_count` this hook ever sees (`0..=20`), the two give identical truncated results (`0.8`'s `double` representation is exact enough that no value in range is within rounding distance of an integer boundary) — see the source comment in `lib.rs` for the full argument. |
| 4 | Every Hook API call is called through `hooks_core`'s raw FFI directly (`src/raw.rs`), not `hooks_lib::api`'s `Result<_, HookError>` wrappers | See "Toolchain limitation" below — a build constraint, not a behavior change. |
| 5 | `n > HOOK_MAX` (`HOOK_MAX = 10`) lets hook topic `n == 10` through, even though only hook slots `0..=9` exist on a 10-element `Hooks` array | Preserved exactly (`HOOK_MAX: u8 = 10`, same `>` comparison) | Matches govern.c's own off-by-one; a vote for topic `H10` records/threshold-checks normally but its *actioning* (`action_hook`) would address a nonexistent 11th slot. Not independently verified against a live node (see "Testing" below) — flagged here rather than silently "fixed." |

No other intentional behavioral differences. The setup sequence, the vote
key/vote-count key overlap-clobber quirk (see `keys.rs`'s doc comments),
the 8-case seat-change logic table, the vote garbage-collection double
loop bounds, and the L1/L2 threshold formulas (`q80`/`q51`, floor `<2`
clamped to `2`) are all transcribed exactly.

## Emitted transactions

Both built in `txn.rs` from `hooks_lib::txn::codec`'s STObject primitives
(the same ones `txn_template!` and `../80_reward/src/mint_txn.rs` use),
**not** `txn_template!` itself (both have shapes it doesn't support: a
variable-position `Hooks` array and, for the `Invoke`, a `Parameters`
array govern.c itself builds from a hardcoded hex byte template rather
than field-by-field — reproduced here byte-for-byte from that same
template rather than re-derived, since it's the most faithful option).
Both use the null (2-byte) `SigningPubKey` encoding, matching
`macro.h`'s `ENCODE_SIGNING_PUBKEY_NULL` (unlike `../80_reward`'s
GenesisMint, which bakes in reward.c's own 35-byte zero-filled form).

- **L1-vote-forward `Invoke`**: `TransactionType(99) | Flags(tfCANONICAL) |
  Sequence(0) | FirstLedgerSequence | LastLedgerSequence | Fee |
  SigningPubKey(empty) | Account(this L2 table) | Destination(genesis) |
  EmitDetails | Parameters[T, V]`.
- **`HookSet`**: `TransactionType(22) | Flags(tfCANONICAL) | Sequence(0) |
  FirstLedgerSequence | LastLedgerSequence | Fee | SigningPubKey(empty) |
  Account(this table) | EmitDetails | Hooks[10]` — 9 no-op `Hook` objects
  (`{}`) plus one `hsfOVERRIDE`'d entry at the voted slot, either
  `CreateCode(empty)` (delete) or `HookHash(the voted hash)` (install).

## Toolchain limitation: `HookError` decoding and nesting depth

See `examples/80_reward`'s README for the full writeup (discovered while
porting that hook first) — the identical constraint applies here, more so
(govern.c is the larger of the two genesis hooks, and its vote-processing
path alone makes more distinct fallible Hook API calls than reward.c's
entire reward computation). Two additional findings from porting this
hook specifically, beyond `examples/80_reward`'s:

- **Floating point is rejected outright, not just costly.** The Hook
  API's guard checker flags any `f64`/`f32` wasm opcode as a hard error
  for a Guard-type hook (`hooks-build` reports "uses a floating-point
  opcode"), independent of the nesting-depth issue. govern.c's `q80`/
  `q51` threshold computation (`member_count * 0.8` as a C `double`) had
  to be replaced with exact integer arithmetic — see the differences
  table above.
- **The number of distinct fallible-call *sites* in one function matters
  as much as each call's own cost.** Beyond the `raw`-module fix
  (`examples/80_reward`'s primary finding), consolidating
  `txn.rs`'s transaction encoders from ~30 individual `push`/
  `push_field_header` calls down to ~10 combined ones (concatenating
  adjacent constant/semi-constant byte fragments into single `push::<N>`
  calls, and eliminating placeholder-then-patch round trips for fields
  already known at write time) took this hook's `hook()` from nesting
  depth 42–56 (rejected) to 22 (comfortably under the 32-level limit) —
  see `txn.rs`'s `write_common_header`'s doc comment for the pattern.
- **`guard!`'s iteration budget is scoped to the static call site for the
  whole hook execution, not to each dynamic loop entry.** Confirmed live,
  via this crate's own e2e suite: `garbage_collect_votes`'s inner loop
  (govern.c's `GUARD(66)`, `for (int i = 0; GUARD(66), i < 32; ++i)`) was
  first ported as `guard!(32)` — a tighter, seemingly-equivalent bound,
  since the loop body itself only ever runs `0..32`. That failed on a
  live node with `GUARD_VIOLATION` (`Macro guard violation... Iterations:
  34`) the moment a seat change actually reached the vote-garbage-
  collection step, because `garbage_collect_votes` runs *twice* per
  `action_seat` call (once for `tbl = 1`, once for `tbl = 2`), and the
  Hook API's guard mechanism tracks iterations *cumulatively* per source
  line for the entire hook invocation — the second call's iterations add
  to the first's against the same guard, not reset. govern.c's own
  `GUARD(66)` (`2 * 32` plus slack) encodes this accumulation correctly
  and was not, as first assumed while porting, simply generous headroom
  — restored verbatim (`guard!(66)`) once the live failure made the
  reason legible. General lesson: a loop-bound Rust port of a C `GUARD(N)`
  should keep `N` as-is whenever the loop's enclosing function can run
  more than once per hook execution, even if the loop's own trip count
  alone would justify a tighter bound.

## Testing

See `e2e/test/govern.test.ts` and the top-level PR description for the
live-node test matrix (against `XahauGenesis_test.cpp`'s governance
coverage) and which cases could/couldn't be reproduced outside a real
genesis-activated network — notably, this repo's e2e suite *can* install
both `govern`/`reward` on the real network genesis account
(`rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh`, secp256k1 seed
`snoPBrXtMeMyMHUVTgbuqAfg1SUTb`/`"masterpassphrase"`) on a standalone
node, since that account's `AccountID` is exactly `GENESIS_ACCOUNT` in
both this crate and `../80_reward` — see the PR description for how this
was confirmed and what it does and doesn't unlock for L1-table testing.
