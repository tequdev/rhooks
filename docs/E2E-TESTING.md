# E2E testing the example hooks against a standalone Xahau node

Status: IMPLEMENTED (Phase 1, 2026-07-24) — `e2e/` package, mise tasks
(`e2e:node-up` / `e2e:node-down` / `e2e`), and `.github/workflows/e2e.yml`.
All four suites pass against a live standalone xahaud
(`2026.6.21-release+3350`); emit-txn's run was the first live validation of
`txn_template!`'s wire format (emitted Payment applied `tesSUCCESS` with the
`cbak` execution recorded). Two deviations from the plan below, both
deliberate:

- **CI provisions via `xrpld-netgen` too**, not the "plain docker +
  generated genesis" variant: hooks-toolkit's `genesis.js` turned out to be
  GPL-3.0 (not MIT), so vendoring it into this MIT repo was off the table,
  and invoking netgen as a CLI keeps local and CI provisioning identical.
- **"live `HookInstructionCount` ≤ static worst-case" is NOT asserted as an
  invariant**: the vendored checker counts syntactic instructions (a host
  `call` costs 1; host-function work is not modeled), while the node's
  runtime meter charges real cost — emit-txn's trivial `cbak` measures 10
  live vs 7 static. The suites assert it for `hook()` bodies (where it held
  with margin) and use a loose sanity bound for `cbak`.

Date: 2026-07-24

`hooks-build check` (and the vendored `Guard.h` checker) prove a binary is
*SetHook-valid*; they prove nothing about runtime behavior. The examples
should also be executed on a real xahaud: deploy via SetHook, trigger,
and assert results from transaction metadata and ledger state. This
document records how the ecosystem (and this machine's own sibling repos)
does that today, and proposes how to do it here.

## 1. How standalone Xahau hook testing is done today

Three sources were surveyed: Transia's `hooks-toolkit-ts` (the de-facto
integration framework), this machine's own hook repos
(`chooks-template`, `gas-chooks-template`, `iou-reward-hook`,
`jshooks-samples` — all by this repo's author), and the provisioning tool
`xrpld-netgen`. Findings:

### Node provisioning

- **`xrpld-netgen up:standalone --protocol xahau --version <tag>`** is the
  local-dev standard in every sibling repo. It downloads the pinned
  `xahaud` binary from `https://build.xahau.tech/<tag>`, bakes it into an
  `ubuntu:jammy` image, and runs `xahaud -a --ledgerfile genesis.json`
  (true standalone). Defaults: container name `xahau`, `network_id`
  **21339**, admin WS **6006**, admin RPC **5005** (public WS 6008 / RPC
  5007), **every amendment of that build pre-enabled at genesis** (it
  parses the build's `features.macro` into the genesis `Amendments`
  entry). Also starts a `transia/explorer` container. **amd64-only**
  (Apple Silicon runs it under emulation; the author already does this
  daily).
- **hooks-toolkit's CI pattern** is the lighter per-run variant: download
  the binary, generate `genesis.json` with a script (master account
  `rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh` pre-funded, amendments enabled),
  then a single
  `docker run --rm -p 6006:6006 -v ... ubuntu:latest xahaud -a --conf ... --ledgerfile genesis.json`.
  No netgen, no explorer — a fresh node per CI run (their pin at the time
  of survey: `2026.6.21-release+3350`, network_id 21337).
- Full-node repos (`mainnet-docker`, `Xahau-Testnet-Docker`,
  `docker-rippled`) are network-syncing nodes — unsuitable for isolated
  tests. `craft` is XRPLF's Smart-Escrow WASM platform (different ABI,
  `wasm32-unknown-unknown`, `finish()` entry) — not applicable.
- Hosted testnets (xahau-test.net) are stateful, shared, and faucet-bound
  — fine for manual smoke tests, wrong for CI determinism.

### Test driver

Every sibling repo uses **pnpm + vitest + `@transia/hooks-toolkit`** (or
the author's own `@tequ/hooks-toolkit` fork) with the `xahau` (xrpl.js
fork) client:

- `XAHAU_ENV=standalone`; connects to `ws://0.0.0.0:6006` (admin WS).
- Signing is **client-side** (`client.autofill` + `client.submit(tx, {wallet})`);
  `NetworkID` is autofilled from `client.networkID`.
- Ledgers are advanced manually with the admin **`ledger_accept`** RPC
  (standalone never auto-closes); hooks-toolkit's `Xrpld.submit` wraps
  submit + `ledger_accept` + `tx` verification, and on `tecHOOK_REJECTED`
  throws the decoded `HookReturnCode: HookReturnString`.
- Deployment: `setHooks`/`setHooksV3` builds the `SetHook` tx from a local
  wasm file (hex-encoded `CreateCode`), `calculateHookOn([...])`,
  namespace, `HookApiVersion`, optional `HookParameters`. Teardown:
  `clearAllHooks` (override + nsdelete).
- Assertions:
  - `ExecutionUtility.getHookExecutionsFromMeta` → `HookReturnCode`,
    `HookReturnString`, `HookInstructionCount`, `HookStateChangeCount`;
  - `StateUtility.getHookState` (`ledger_entry hook_state`) and
    `account_namespace` for state;
  - `meta.HookEmissions[].EmittedTxnID` → `ledger_accept` → `tx` to
    confirm the emitted txn applied (and its own `cbak` execution);
  - deterministic wallet roster funded from the genesis master account
    (seed `snoPBrXtMeMyMHUVTgbuqAfg1SUTb`).
- trace() output: on standalone it lands in the node's debug log — the
  sibling repos read it with `docker logs -f xahau`. (The public
  `debugstream` service exists only for hosted testnets.)
- Tests run serially (`fileParallelism: false`) because they share ledger
  state.

### Rust-native alternative (assessed, deferred)

A pure-Rust harness (no Node dependency) is feasible in principle because
standalone admin RPC supports server-side signing (`submit` with `secret`
+ `tx_json`), which would reduce the client to JSON-RPC + serde — no
binary codec or signing crates. No maintained Rust client knows Xahau's
`SetHook`/Hook fields today, so *client-side* signing in Rust would mean
maintaining a custom definitions fork — not worth it. Even with
server-side signing, a Rust harness re-implements what hooks-toolkit
already provides (HookOn calculation, execution/state/emission decoding,
retry and ledger_accept choreography). Assessment: possible later, not
the place to start; the `submit`-with-`secret` path should get a small
verification spike before anyone commits to it.

## 2. Proposal for this repo

**Phase 1 (recommended): an `e2e/` directory using the author's existing
stack** — pnpm + vitest + hooks-toolkit + `xahau` — because it is the
established pattern in every sibling hook repo on this machine, and the
assertion toolkit (executions, state, emissions) already exists there.
The one difference from the sibling repos: the wasm under test comes from
**`hooks-build`** (`examples/*/out/*.wasm`), not from `c2wasm-cli` — the
e2e suite is as much a test of the toolchain as of the examples.

Sketch (not implemented):

```
e2e/
├── package.json          # pnpm; vitest; @transia/hooks-toolkit; xahau
├── .env                  # XAHAU_ENV=standalone
└── test/
    ├── accept-all.test.ts
    ├── firewall.test.ts
    ├── state-counter.test.ts
    └── emit-txn.test.ts
```

- mise tasks: `e2e:node-up` (`xrpld-netgen up:standalone --protocol xahau
  --version <pinned>`), `e2e:node-down`, `e2e` (`mise run build-examples`
  → `pnpm vitest` in `e2e/`). Node version pinned in one place.
- Per-example assertions:
  - **accept-all**: Invoke → `HookExecutions[0]` accept, return string.
  - **firewall**: install with `BL` HookParameter = blocked AccountID;
    Payment from the blocked account → `tecHOOK_REJECTED` (decoded
    message asserted); from another account → success.
  - **state-counter**: two Invokes → `ledger_entry hook_state` shows the
    LE counter = 2; accept return code carries the count.
  - **emit-txn**: Invoke → `HookEmissions` has an `EmittedTxnID`;
    `ledger_accept`; the emitted Payment applies `tesSUCCESS` (1 drop to
    the original sender) and its metadata shows the `cbak` execution.
    **This closes the standing caveat that `txn_template!`'s wire format
    has never been exercised against a live node.**
  - Cross-check: `HookInstructionCount` ≤ the worst-case count
    hooks-build printed; SetHook fee from `hooks-build`'s estimate is
    accepted by the node.
- CI: a separate `e2e` workflow job on ubuntu (amd64 — no arch problem),
  using the hooks-toolkit CI pattern (plain `docker run` + generated
  genesis, no netgen/explorer overhead), gated after build-test. Runtime
  budget ~2-4 min including node startup. Initially non-required while it
  proves stable.
- Local dev (Apple Silicon): `xrpld-netgen` under Docker Desktop/colima
  with amd64 emulation — the author's daily-driver setup already.

**Phase 2 (optional, later)**: revisit a Rust-native harness behind the
same mise task interface once the server-side-signing spike is done, if
dropping the Node dependency ever becomes worth the re-implementation.

## 3. Open points to settle at implementation time

1. Which xahaud version to pin (sibling repos use recent
   `-release+` builds; must be ≥ the HooksUpdate2 features the examples
   rely on, and ideally near the vendored `release`-branch checker).
2. netgen (21339) vs hooks-toolkit-CI genesis (21337) network id — either
   works since `NetworkID` is autofilled from the node; pick one for both
   local and CI to keep configs identical.
3. Whether `e2e/` shares the repo's pnpm workspace tooling or stays a
   fully isolated package (leaning isolated: it is a test rig, not a
   publishable artifact).
4. Whether to enable the `trace` feature in example dev-builds for the
   e2e suite and assert on `docker logs` trace lines (nice-to-have;
   brittle — probably manual-only).
