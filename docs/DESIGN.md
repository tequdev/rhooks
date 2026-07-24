# rhooks — Rust Library & Toolchain for Xahau Hooks

Status: REVIEWED — rework findings from the external design review (Codex
gpt-5.5, reasoning effort high, 2026-07-23) have been incorporated; see §11.
Author: design by Claude (Fable 5); implementation delegated per phase
Date: 2026-07-23

## 1. Goals

Provide a Rust monorepo for developing Xahau Hooks (WebAssembly smart
contracts) end to end:

1. **hooks-core** — zero-logic FFI layer: raw Hook API declarations and every
   constant from the xahaud `hook/` headers (`error.h`, `extern.h`,
   `ls_flags.h`, `sfcodes.h`, `tts.h`, `tx_flags.h`, keylet/compare constants
   from `hookapi.h`), translated 1:1 into Rust.
2. **hooks-lib** — ergonomic, Rust-idiomatic wrapper over hooks-core
   (`Result`-based APIs, typed buffers, XFL type, guard/trace macros, panic
   handler). This is the crate Hook developers import.
3. **hooks-build** — CLI that turns a Rust crate into a SetHook-valid WASM:
   drives `cargo build --target wasm32v1-none`, then performs the
   hook-cleaner and guard-checker steps natively in Rust.
4. **examples** — multiple working Hooks written with hooks-lib, buildable
   with hooks-build.

### Non-goals (v1)

- Publishing to crates.io (names/ownership decided later; `publish = false`).
- Gas-type hook (HookApiVersion 1) *ergonomics*. The pipeline accepts
  `--api-version 1` (skips guard handling) but hooks-lib v1 targets
  Guard-type hooks.
- Deployment tooling (SetHook submission, faucet, networks). Out of scope;
  hooks-build stops at a valid `.wasm` plus a fee estimate.
- WAT round-tripping, debugger, simulator.

## 2. Constraints that shape the design

These come from xahaud's SetHook validation (`SetHook.cpp`,
`validateHookSetEntry`) and prior experience building the same toolchain:

- **C1. Export set**: the WASM may export only `hook` (and optionally
  `cbak`), both `(func (param i32) (result i64))`. Rust `cdylib` output also
  exports `memory` — it must be stripped, or SetHook fails `temMALFORMED`.
- **C2. Guards**: for API version 0, every `loop` must begin with the exact
  instruction sequence `i32.const <id>; i32.const <maxiter>; call $_g`
  (result dropped). xahaud statically computes a worst-case instruction
  count from these; missing guards ⇒ rejection.
- **C3. Instruction set**: WASM 1.0 MVP only. No bulk-memory, sign-ext,
  reference types, SIMD; **no floating-point opcodes at all**. Rust target
  `wasm32v1-none` guarantees the MVP feature set but not float-freedom —
  float ops must never appear in source (XFL math is done via host calls).
- **C4. Imports**: only the documented Hook API functions plus `_g`, all
  from module `env`. Anything else ⇒ rejection.
- **C5. No recursion**: the static instruction-count analysis requires an
  acyclic call graph.
- **C6. Size**: ≤ 65,535 bytes; SetHook fee ≈ 5000 drops/byte, so every
  byte matters. No allocator, no `core::fmt`, no panic machinery.
- **C7. Panic machinery is poison**: slice bounds checks pull in panic paths
  that add functions/calls and have historically broken validation.
  hooks-lib must be panic-free by construction (no indexing that can
  panic in release; caller-provided buffers; `Result` everywhere).
- **C8. Byte-exact post-processing**: the post-processor must re-encode the
  module without disturbing the guard byte pattern. Use
  `wasmparser` + `wasm-encoder` (raw section copy where possible);
  **walrus is deliberately avoided** — its IR round-trip does not preserve
  instruction sequences byte-exactly.

## 3. Repository layout

```
rhooks/
├── Cargo.toml                # workspace: crates/* (examples excluded)
├── rust-toolchain.toml       # stable channel + wasm32v1-none target
├── rustfmt.toml
├── mise.toml                 # fmt / lint / test / build-examples tasks
├── .gitignore                # target/, out/, *.wasm artifacts
├── docs/
│   └── DESIGN.md             # this file
├── crates/
│   ├── hooks-core/           # no_std, FFI decls + constants, no logic
│   ├── hooks-macros/         # std, proc-macro crate (#[hook]/#[cbak], txn_template! internals)
│   ├── hooks-lib/            # no_std, idiomatic wrapper (depends: hooks-core, hooks-macros)
│   ├── hooks-build/          # std, bin+lib CLI (clap, wasmparser, wasm-encoder)
│   └── xtask/                # std, bin CLI: header → hooks-core codegen
└── examples/
    ├── Cargo.toml            # SEPARATE workspace (no_std cdylibs)
    ├── 01_accept-all/        # numbered = suggested reading order
    ├── 02_state-counter/     # (package names are unprefixed - Cargo
    ├── 03_hook-params/       # package names can't start with a digit)
    ├── 04_errors/
    ├── 05_firewall/
    ├── 06_guard-patterns/
    ├── 07_xfl-math/
    ├── 08_slot-ledger/
    ├── 09_state-foreign/
    └── 10_emit-txn/
```

- Root workspace members: `crates/*` only. `examples/` is its own workspace:
  its crates are `no_std` cdylibs with hook-specific release profiles that
  must not leak into host crates, and they don't build for host targets.
- Edition 2024, `rust-version = "1.85"` (wasm32v1-none is stable ≥ 1.84). A
  stable toolchain is pinned via `rust-toolchain.toml` (currently `1.89.0`,
  matching `mise.toml`'s `[tools] rust` pin — see §5.5 for why no nightly
  feature is needed: `hooks-macros`, a small hand-rolled `proc_macro` crate,
  covers what `${concat(...)}` used to); `rust-version` still tracks the
  language edition floor, not the exact pinned toolchain.
- All crates `publish = false` for now.
- All comments, docs, and identifiers in English.

## 4. hooks-core

`#![no_std]`, zero dependencies, zero logic. A faithful, mechanical
translation of the headers. Layout:

```
src/
├── lib.rs        # crate docs, module wiring, re-exports
├── api.rs        # extern "C" declarations (the 60+ Hook API fns + _g)
├── error.rs      # error.h     → pub const SUCCESS: i64 = 0; OUT_OF_BOUNDS = -1; ...
├── sfcodes.rs    # sfcodes.h   → pub const sfAccount: u32 = ...; (325 consts)
├── tts.rs        # tts.h       → pub const ttPAYMENT: u32 = 0; ...
├── ls_flags.rs   # ls_flags.h  → pub const lsfGlobalFreeze: u32 = ...;
├── tx_flags.rs   # tx_flags.h  → pub const tfFullyCanonicalSig: u32 = ...;
└── consts.rs     # hookapi.h + macro.h constant-like defines
```

`consts.rs` covers every *constant-like* define from `hookapi.h` and
`macro.h`: `KEYLET_*` (1–26), `COMPARE_*`, `tfCANONICAL`, the `atACCOUNT`
family (amount/account offset constants), and the `amAMOUNT` family.
Function-like macros in `macro.h` (`SBUF`, `BUFFER_EQUAL`, …) are C
conveniences and are NOT ported here — their roles are covered by hooks-lib.

Rules:

- **Names are kept verbatim** (`sfAccount`, `ttPAYMENT`, `lsfGlobalFreeze`,
  `OUT_OF_BOUNDS`) under `#![allow(non_upper_case_globals)]` so code can be
  grepped against C hooks and the official docs. No renaming, no typing
  cleverness at this layer.
- Types: error codes `i64` (they are compared against Hook API `i64`
  returns); `sfcodes` `u32`; `tts` `u32`; flags `u32`.
- The extern block mirrors `extern.h` exactly — `read_ptr`/`read_len` style
  `u32` parameters, `i64` returns:

```rust
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    pub fn _g(guard_id: u32, maxiter: u32) -> i32;
    pub fn accept(read_ptr: u32, read_len: u32, error_code: i64) -> i64;
    pub fn state(write_ptr: u32, write_len: u32, kread_ptr: u32, kread_len: u32) -> i64;
    // ... every function from extern.h, in extern.h order
}
```

- **Host builds**: the extern block is `#[cfg(target_arch = "wasm32")]`.
  For other targets, same-signature deterministic stub fns are provided so
  hooks-lib and its docs/tests compile *and run* on the host: every stub
  returns `NOT_IMPLEMENTED` (no panicking `unimplemented!()`). A richer
  feature-gated mock host is possible later without changing this surface.
- Each declaration carries a one-line doc comment naming the C prototype;
  a header comment records the upstream source
  (`Xahau/xahaud`, branch `release`, `hook/<file>.h`) so re-generation diffs
  are reviewable.
- **The source headers are vendored, not just referenced**: all eight
  `hook/*.h` files live verbatim in `crates/hooks-core/vendor/xahaud-hook/`
  (own `VENDOR.md` + `SHA256SUMS`), synced by the same
  `scripts/sync-vendor.sh` / weekly drift workflow as the guard checker
  (6.5). **Parity tests** in hooks-core parse the vendored headers at test
  time (C `#define`/enum extraction with a tiny shift-add expression
  evaluator, and `extern.h` prototype parsing) and compare complete
  name/value/signature sets against the Rust translation — so an upstream
  header change first fails the drift workflow, and after re-syncing, the
  parity tests fail until the Rust side is updated to match. The
  translation cannot silently rot.
- **The translation itself is generated**: `cargo xtask gen-core`
  (crates/xtask) parses the vendored headers and emits all of hooks-core's
  translated sources (`error.rs`, `tts.rs`, `sfcodes.rs`, `ls_flags.rs`,
  `tx_flags.rs`, `consts.rs`, `api.rs` — everything except the hand-written
  `lib.rs`), each carrying an `@generated` marker. `gen-core --check`
  verifies the checked-in sources match regeneration (wired into CI), so
  the full sync flow is: `scripts/sync-vendor.sh` → `cargo xtask gen-core`
  → tests → commit. The xtask parser is deliberately independent from the
  parity tests' parser — the parity tests are the generator's correctness
  oracle, so they must not share code.

## 5. hooks-lib

`#![no_std]`, depends only on hooks-core. `#![deny(missing_docs)]`.

```
src/
├── lib.rs         # prelude, panic handler (feature), re-export of hooks-core as `raw`
├── error.rs       # HookError + Result<T>
├── types.rs       # AccountId, Hash, Keylet, ... #[repr(transparent)] fixed-size newtypes
├── convert.rs     # ToBytes/FromBytes boundary conversion traits
├── state.rs       # typed state layer (state_get/state_set_typed/state_update_typed) + state_keys!
├── buf_eq.rs      # loop-free, panic-free fixed-size buffer equality (buf_eq_8/20/32/...)
├── errors.rs      # hook_errors! user error enum -> rollback code mapping
├── xfl.rs         # XFL newtype over i64
├── txn.rs         # txn_template! macro + generic field-encoding primitives
├── static_cell.rs # HookStatic: take-once cell for static hook buffers
├── macros.rs      # guard!, trace!, rollback!, accept!, pad!
└── api/
    ├── mod.rs
    ├── control.rs # accept, rollback (-> !), hook_again, hook_skip, hook_pos
    ├── otxn.rs    # otxn_field, otxn_type, otxn_param, otxn_id, otxn_slot, ...
    ├── state.rs   # state, state_set, state_foreign(_set)
    ├── etxn.rs    # etxn_reserve, emit, etxn_details, etxn_fee_base, prepare
    ├── ledger.rs  # ledger_seq, ledger_last_time, fee_base, ledger_keylet, ...
    ├── hook_ctx.rs# hook_account, hook_hash, hook_param(_set)
    ├── slot.rs    # slot_* family, meta_slot, xpop_slot
    ├── sto.rs     # sto_subfield, sto_subarray, sto_emplace, sto_erase, sto_validate
    ├── float.rs   # thin fns backing XFL (float_sto, float_sto_set, slot_float)
    ├── util.rs    # util_accid, util_raddr, util_sha512h, util_verify, util_keylet
    └── trace.rs   # trace, trace_num, trace_float
```

### 5.1 Error model

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookError {
    OutOfBounds,          // -1
    InternalError,        // -2
    TooBig,               // -3
    /* ... every code from error.h ... */
    Unknown(i64),         // forward-compat for codes we don't know
}
pub type Result<T> = core::result::Result<T, HookError>;

#[inline(always)]
fn res(code: i64) -> Result<i64> { if code < 0 { Err(HookError::from(code)) } else { Ok(code) } }
```

Non-negative returns are payload (usually "bytes written"); negative maps to
`HookError`. Functions whose success value is meaningful keep it
(`Ok(len)`, `Ok(slot_no)`, …).

### 5.2 API wrapper conventions

- Caller-provided buffers, length returned — zero-copy and panic-free:

```rust
#[inline(always)]
pub fn state(out: &mut [u8], key: &[u8]) -> Result<usize>;
#[inline(always)]
pub fn hook_account(out: &mut [u8]) -> Result<usize>;
#[inline(always)]
pub fn hook_account_buf() -> Result<AccountId>;   // fixed-size convenience
```

- Every wrapper is `#[inline(always)]` (extra internal functions are both a
  size cost and a validation risk — C7).
- Buffers that have a protocol-fixed size get typed convenience wrappers
  returning `types.rs`' `#[repr(transparent)]` newtypes (`AccountId`
  wrapping `[u8; 20]`, `Hash`/`Keylet`/`Nonce` wrapping `[u8; 32]`/
  `[u8; 34]`/`[u8; 32]`, …) rather than bare arrays — same layout, size,
  and FFI-compatibility as the array, but distinct at the type level so an
  `AccountId` and a `Hash` can no longer be passed to each other's slots by
  accident (see `types.rs`'s module doc comment). The caller-buffer form keeps
  the standard name (`hook_account(out: &mut [u8], ...) -> Result<usize>`,
  matching the raw Hook API's write_ptr/write_len shape and the crate's
  other caller-buffer functions like `state`); the array-returning
  convenience is the same name with a `_buf` postfix
  (`hook_account_buf() -> Result<AccountId>`), for callers who just want
  the value. Writing directly into an existing buffer (e.g. a region of a
  larger template) uses the standard form; the host's own
  TOO_SMALL/OUT_OF_BOUNDS handling applies to whatever slice is passed. The
  `_buf` form delegates to the standard form so each raw call site exists
  once.
- **"as-int64" mode** (`state`, `state_foreign`, `otxn_field`, `slot`):
  the host treats `write_ptr = 0, write_len = 0` as a request to return
  the data itself, packed **big-endian** into the non-negative `i64`
  return — only for data of at most 8 bytes with the top bit clear, else
  `TOO_BIG` (xahaud `applyHook.cpp`, `data_as_int64`). Exposed as
  `<name>_u64(...) -> Result<u64>` variants. (`state_set` /
  `state_foreign_set` have no such mode — they carry no write buffer.)
  Emit details are variable-length — 116 bytes, or 138 when the
  module exports `cbak`
  (verified against `HookAPI::etxn_details` in xahaud) — so there is no
  fixed `EmitDetails` array alias, only `EMIT_DETAILS_MAX_LEN = 138` and a
  caller-buffer `etxn_details(out: &mut [u8]) -> Result<usize>` wrapper.
  (The initial design said 105 bytes; that number was wrong.)
- `accept`/`rollback` return `!` (call, then `unreachable` opcode — the host
  never returns from them).
- Slot/keylet numbers are plain `u32` in v1 (no newtype ceremony); field
  codes are `u32` taken from `hooks_core::sfcodes`.
- **Pointer-direction discipline**: wrappers call the raw extern functions
  directly, spelling out `buf.as_mut_ptr() as u32` for `write_ptr` and
  `buf.as_ptr() as u32` for `read_ptr` at each call site. No generic
  "pass a slice" helper that erases direction — prior art has had bugs from
  exactly that blur (e.g. around `hook_hash`/`hook_skip`). If helpers are
  used at all they must be direction-specific (`out_buf!` vs `in_buf!`).

### 5.3 XFL

`#[derive(Clone, Copy)] pub struct XFL(i64);` — Xahau 64-bit decimal float.
The inner field is **private**: XFL host calls return negative values as
error codes, and a public field would let users smuggle an error code in as
a "value". Escape hatches are explicit: `XFL::from_raw_bits(i64)` /
`xfl.raw_bits()` (documented as unchecked representation access).
All arithmetic goes through host calls and is fallible, so **no `core::ops`
operator overloads** (they would need to panic). Methods:

```rust
impl XFL {
    pub fn new(exponent: i32, mantissa: i64) -> Result<XFL>;      // float_set
    pub fn one() -> XFL;
    pub fn mul(self, rhs: XFL) -> Result<XFL>;                     // float_multiply
    pub fn add(self, rhs: XFL) -> Result<XFL>;                     // float_sum
    pub fn div(self, rhs: XFL) -> Result<XFL>;
    pub fn neg(self) -> Result<XFL>; pub fn invert(self) -> Result<XFL>;
    pub fn mulratio(self, round_up: bool, num: u32, den: u32) -> Result<XFL>;
    pub fn mantissa(self) -> Result<i64>; pub fn exponent(self) -> Result<i64>;
    pub fn sign(self) -> Result<bool>;
    pub fn to_int(self, decimal_places: u32, absolute: bool) -> Result<i64>;
    pub fn compare(self, rhs: XFL, mode: u32) -> Result<bool>;     // float_compare
    pub fn log(self) -> Result<XFL>; pub fn root(self, n: u32) -> Result<XFL>;
}
```

`PartialEq`/`PartialOrd` are NOT implemented (comparison is a fallible host
call, and a silent "false on error" comparison is too dangerous for
financial logic); `compare` plus `eq/lt/gt` helper methods returning
`Result<bool>`. Bitwise representation equality, if ever needed, gets an
explicitly named method (`bits_eq`), not `==`.

### 5.4 Macros & entry point

- `guard!(maxiter)` / `guard_m!(maxiter, n)` — match the C `GUARD`/`GUARDM`
  macros from `macro.h` **exactly, including the `+ 1`**:
  `GUARD(maxiter)` in C is `_g((1ULL << 31U) + __LINE__, (maxiter) + 1)`.
  Rust: `guard!(m)` → `_g((1u32 << 31) + line!(), (m) + 1)`;
  `guard_m!(m, n)` → `_g((1u32 << 31) + (line!() << 16) + (n), (m) + 1)`
  (same id formula as C `GUARDM`) for multiple loops on one line. All
  arithmetic explicit `u32` with `wrapping_add`-free constants.
  Guards are the developer's responsibility by default (see 6.3); the
  opt-in auto-guard pass exists mainly for compiler-generated loops.
- `trace!("msg")`, `trace!("msg", data)`, `trace_num!`, `trace_float!` —
  compiled to nothing unless **hooks-lib's** `trace` feature is enabled
  (traces cost bytes and execution; examples enable it in dev). The feature
  gate lives in hidden `#[inline(always)]` shim functions inside hooks-lib,
  NOT as a `#[cfg]` in the macro body — a cfg written in a `macro_rules!`
  body is evaluated against the *calling* crate's features, which would
  force every hook crate to re-declare a same-named feature. With the shim,
  `hooks-lib = { features = ["trace"] }` on the dependency line is all a
  hook crate needs.
- `accept!()/accept!(msg, code)`, `rollback!(msg, code)` — terse exits.
- `uninit_buf!()` is NOT provided: `MaybeUninit::uninit().assume_init()` for
  arrays is UB. Buffers are `[0u8; N]`; the cleaner/opt pipeline keeps the
  cost acceptable, and correctness wins.
- Entry point: `#[hook]` / `#[cbak]` (from `hooks-macros`, re-exported as
  `hooks_lib::hook`/`hooks_lib::cbak`) turn a plain, argument-less
  `fn name() -> i64` into the required wasm export:

```rust
use hooks_lib::hook;

#[hook]
fn my_hook() -> i64 { ... }
```

  expands to (unchanged original function, plus):

```rust
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    my_hook()
}
```

  `#[cbak]` is identical except it exports `cbak`. Both are hand-rolled
  `proc_macro` (no `syn`/`quote` — see `hooks-macros`'s crate doc comment
  for why): they only ever need to recognize one token shape (a
  no-argument, `i64`-returning, non-generic, non-`async`/`unsafe`/`const`/
  `extern` `fn`), so a general Rust-item parser is unneeded weight. Every
  malformed shape is a `compile_error!` at the offending token, not a
  macro panic.
- Panic handler behind default feature `panic-handler`:
  `rollback(b"panic", ...)` then `unreachable` — examples just work; users
  embedding differently can disable it.

### 5.5 Emitted-transaction templates: `txn_template!` (user-defined layouts)

Modeled on xahaud's C "Tx Builder" split, where the template bytes and
field pointers are pasted into the *hook's own source* and only generic
helpers (`SET_UINT32`, `SET_NATIVE_AMOUNT`, `COPY_20`) are shared: a
library-owned fixed template type (the first iteration's
`PaymentTemplate`) was rejected because any new field or transaction type
would require a hooks-lib release. Instead hooks-lib provides exactly two
things:

1. **Generic encoding primitives** (`txn::codec`): native-amount encoding
   (62-bit check + the `0x40` native bit), big-endian u32 writes, STObject
   field-header derivation from an `sfXxx` code (`type = code >> 16`,
   `field = code & 0xFFFF`; 1–3 prefix bytes per the canonical rules —
   verified against the C template bytes: `0x12`, `0x22`, `0x20 0x1A`,
   `0x61`, `0x73 0x21`, `0x81 0x14`), all `const fn` where layout-relevant.
2. **`txn_template!`** — a declarative macro playing the role of the C
   code generator's output. The hook author declares an ordered field list
   (kinds: `u32_field(sfXxx)`, `native_amount(sfXxx)`, `account_id(sfXxx)`,
   `empty_vl(sfXxx)`, `emit_details`, plus the leading
   `transaction_type = ttXXX`); the macro computes cumulative offsets and
   total length at compile time, bakes the field headers into a
   `const fn new()` template (⇒ data segment via `HookStatic`), and
   generates typed `set_<field>` setters plus an `emit_details_region()`
   accessor. Setter names are synthesized by splicing `set_` and the field
   name (`[<set_ $field>]`) through `hooks-macros`'s `paste`-equivalent
   proc-macro (`$crate::__paste!`, wrapping the generated `impl` block) —
   a small, purpose-built identifier-concatenation macro that replaces
   nightly's `${concat(set_, $field)}` metavariable expression, letting
   `txn_template!` (and every crate that calls it) build on stable Rust.
   A compile-time assertion rejects field lists that violate canonical
   (type, field) ordering — a safety the C flow lacks. `emit_details`
   must be last.

The `PREPARE_TXN()` equivalent, `prepare_for_emit()`, is **generated by
the macro too**, and the emit-plumbing fields are recognized **by their
`sfXxx` code, not by special declaration syntax** — every field uses the
same uniform kinds (`u32_field(sfXxx)`, `native_amount(sfXxx)`,
`account_id(sfXxx)`, `empty_vl(sfXxx)`, `emit_details`). (An earlier
role-kind design — `sequence: sequence,` next to
`flags: u32_field(sfFlags)` — was rejected as a second declaration
dialect users had to learn.)

Mechanically, the muncher accumulates a const table of
`(sfcode, kind tag, payload offset)` per field. The base arm then emits
const-evaluated checks (all failures are named E0080 compile errors):

- **presence**: `sfSequence`, `sfFirstLedgerSequence`,
  `sfLastLedgerSequence`, `sfFee`, `sfSigningPubKey`, `sfAccount` must
  each appear in the table, and an `emit_details` field must be declared
  (last); `transaction_type` is grammar-mandatory and first. An emitted
  transaction without these is invalid at the protocol level, so the
  macro refuses to build one.
- **kind agreement**: the required codes must be declared with the right
  kind (`sfFee` as `native_amount`, `sfAccount` as `account_id`,
  `sfSigningPubKey` as `empty_vl`, the three sequence fields as
  `u32_field`) — a wrong kind would make `prepare_for_emit` corrupt the
  template, so it is rejected at compile time.

Because detection is by *value*, it is robust to how the constant is
spelled (qualified paths, aliases). `prepare_for_emit(&mut self) ->
Result<Prepared<'_, Self>>` is generated unconditionally, resolving the six
offsets by const lookup in the same table (`ledger_seq()+1`→FLS, FLS+4→LLS,
`hook_account()`→account, `etxn_details` into the region — its returned
length fixes the real blob length — then `etxn_fee_base` over the actual
blob→fee). `Prepared<'a, T>` (`hooks_lib::txn::Prepared`) is a typestate
wrapper — `{ inner: &'a mut T, len: usize }` — that is the *only* way to
reach an emit-sized slice (`Prepared::as_bytes`) or emit it
(`Prepared::emit`, wrapping `api::etxn::emit_buf`): the unprepared template
type has no `as_bytes`/`emit` method at all, so code cannot emit a buffer
whose FLS/LLS/Account/EmitDetails/Fee were never actually filled — that
mistake is now a compile error (`E0599`, no method found), not a runtime
footgun. `Prepared` borrows rather than owns `Self` (generated structs
usually live behind `HookStatic::take`'s `&'static mut T`, so an owning
typestate would need a needless `mem::replace` dance) and `Deref`/
`DerefMut`s to `Self`, so setters remain callable after preparing too (e.g.
adjust a field and call `prepare_for_emit` again). Setters are generated
uniformly for every settable field regardless of role — value-based
required-field detection cannot be reflected in which setters *exist*, only
in what a separate typestate lets you do with them — so setter existence
is unchanged; only the FLS/LLS/Account/EmitDetails/Fee values themselves
are inaccessible for emission until `prepare_for_emit` runs. Transaction
*shape* remains entirely user-declared — new fields or txn types never
require a hooks-lib change; only the fixed emit plumbing is canned. The
`const fn new()` template always reserves
the full `EMIT_DETAILS_MAX_LEN = 138` bytes of capacity for the
emit-details region regardless of whether the module exports `cbak`, but
those reserved zero bytes cost nothing in the emitted binary — the
cleaner's trailing-zero data-segment trim (6.2 step 3) strips them from
the baked template's data segment — and the *runtime* blob length is
whatever `etxn_details` actually returns (116 bytes without `cbak`, 138
with), so cbak-vs-not needs no declaration-time switching at all.

## 6. hooks-build

`std` crate: `src/main.rs` (clap CLI) + `src/lib.rs` (pipeline as pure
`bytes → Result<bytes>` functions, unit-testable). Dependencies: `clap`,
`wasmparser`, `wasm-encoder`, `anyhow`; dev-dep `wat` for fixtures.
No walrus (C8).

### 6.1 CLI

```
hooks-build build [--manifest-path <dir/Cargo.toml>] [-p <crate>]
                  [--api-version 0|1] [--auto-guard] [--default-maxiter N]
                  [--out <dir>] [--allow-oversize]
hooks-build clean <in.wasm> [-o out.wasm] [--api-version 0|1]   # post-process only
hooks-build check <file.wasm> [--api-version 0|1]               # validate only, no output
```

`build` =
1. `cargo build --release --target wasm32v1-none` with
   `--message-format=json` to locate the produced `.wasm` artifact (no
   guessing at target dirs).
2. Post-process (6.2, 6.3).
3. Validate (6.4).
4. Write `<out>/<crate>.wasm` (default `out/` beside the manifest), print
   size and estimated SetHook fee (`bytes × 5000` drops).

`check` runs only 6.4 (+ guard verification instead of insertion) — usable
against any wasm, including C-built hooks.

### 6.2 Cleaner (hook-cleaner equivalent)

Input: cargo's wasm. Output: SetHook-shaped wasm.

1. Drop all custom sections (`name`, `producers`, `target_features`, …).
2. Restrict exports to exactly `hook` and (if present) `cbak`; everything
   else — `memory`, `__wasm_call_ctors`, data/table globals — is removed.
3. Reachability GC. Roots: the retained exports only (`call_indirect` is
   rejected in v1 — see 6.4 — so table element segments are never roots;
   a `start` section is rejected outright). Traversal follows direct
   `call` instructions and `global.get/set`. Then:
   - drop unreferenced functions and globals;
   - drop the table and all element segments entirely when no
     `call_indirect` survives (v1: always, given the hard error);
   - trim every active data segment's payload to end at its last non-zero
     byte (dropping the segment entirely if it is all zero) — wasm linear
     memory is zero-initialized by definition, so trailing zero bytes are
     pure dead weight at 5000 drops/byte; only the payload shrinks from
     the tail, the offset expression is untouched, and this preserves
     semantics since memory size comes from the memory section, not
     segment lengths. **Safety guard**: active segments apply in
     declaration order and may legally overlap, in which case a trailing
     zero can be a deliberate overwrite of an earlier segment's non-zero
     byte — so the trim runs only when every offset is a plain
     `i32.const` and no two segment ranges intersect (LLVM/wasm-ld never
     emit overlaps, but `clean` accepts arbitrary wasm). Segment
     *merging* remains a future optimization; a live defined memory is
     required whenever at least one (untrimmed) segment survives.
4. **Index renumbering is a whole-module concern.** One remap table per
   index space (types, functions, globals, memories, tables) is built once
   and applied everywhere that space is referenced: function section,
   export section, element segments, code bodies (`call` immediates,
   `global.get/set` immediates), and import ordering (imported functions
   occupy the low indices, so adding/removing an import — e.g. `_g` —
   shifts every defined function index). A code body may be raw-copied
   **only if no index space it references changed**; otherwise it is
   re-encoded instruction-by-instruction with immediates remapped.
   Byte-comparison tests pin the re-encoder as lossless modulo remapped
   immediates (C8).
5. Verify entry signatures: `hook`/`cbak` must be `(i32) -> i64`; error out
   otherwise (catches a missing `extern "C"` or wrong signature early).

### 6.2b Flatten pass (full inlining) — api-version 0

Two rules of the real checker (`Guard.h`, discovered by running the vendored
checker against our phase-4 artifacts, which the Rust reimplementation had
wrongly accepted):

- **R1**: every api-version-0 module must import `_g`, even if it contains
  no loop at all.
- **R2**: every entry in the type section must be the type of an import or
  the `(i32) -> i64` entry-point type. A defined helper function with any
  other signature — notably `compiler_builtins` `memset`/`memcpy`/`bcmp`
  (`(i32,i32,i32) -> i32`), which rustc emits for large buffer zero-inits
  and array comparisons — makes the whole module invalid.
  (`#![no_builtins]` does not prevent these under fat LTO; verified
  empirically. Source-level avoidance is not reliable.)

Consequently the cleaner is followed by a **flatten pass** for api-version
0: inline every defined non-entry function into its callers, bottom-up in
topological order (the call graph is acyclic — recursion is banned — so
this terminates), then drop the inlined functions and rebuild the type
section to exactly {import types} ∪ {entry type}. Inlining transform per
call site: arguments are spilled to fresh locals, the callee body is
spliced in wrapped in a `block` of the callee's result type, callee locals
are remapped to appended caller locals, and every `return` in the callee
becomes a `br` to the wrapper block (branch depths inside the body shift by
one accordingly). Multiple call sites duplicate the body — a size cost that
is acceptable and reported. `_g` is ensured present as an import for
api-version 0 (added if absent, never GC'd) per R1.

Inlining wraps a call site in a `block` only when the callee body actually
contains a non-trailing `return` (the block exists solely as the rewritten
`br` target); a trailing `return` is dropped and falls through, and
return-free bodies are spliced bare.

### 6.2c Unnest pass (ladder flattening) — api-version 0

`Guard.h` rejects modules whose block nesting exceeds **32 levels**
(`NESTING_LIMIT`, 16 before `GuardRuleDepth32`) during its worst-case
analysis. LLVM's stackifier lays out every diverging early-exit
(`rollback!`-style) as a tail after the end of a dedicated `block` wrapping
the whole remaining body — so nesting grows linearly with the number of
error paths (the "error ladder"), and a hook with a few dozen checks would
exceed the limit regardless of guards.

The unnest pass runs after flatten and exploits the fact that those tails
are **self-contained and diverging** (push constants, `call rollback`,
`unreachable` — consuming nothing from the outer stack):

1. **Diverging-tail duplication**: a `br_if` targeting a block whose
   continuation is such a tail is rewritten to `if` + the tail spliced
   inline (an unconditional `br` gets the tail spliced directly). The tail
   is verified self-contained by symbolic stack simulation (starts empty,
   never underflows, only constants / `local.get` / import calls / `drop`,
   ends in `unreachable`); only empty-blocktype blocks qualify.
2. **Unreferenced-block unwrapping**: any empty-blocktype block no longer
   targeted by any branch is removed, with branch-depth immediates inside
   it fixed up. This also erases flatten wrapper blocks whose `return`
   rewrites never materialized.
3. Iterate to fixpoint (ladders unwrap outermost-inward).

The local `if` costs one level only inside a short error arm, while each
removed ladder block spanned the entire function — net max-depth drops from
O(error paths) to O(real control structure). The duplicated tails cost a
few bytes per branch site. Correctness is held by the same wasmi
differential harness as flatten (identical results and host-call
sequences pre/post pass). The validator (6.4) additionally computes max
nesting depth, hard-erroring above 32 for api-version 0 (mirroring
`GuardRuleDepth32`) and warning at ≥ 28; `build` prints the final depth.

The guard pass (6.3) runs **after** flattening and unnesting, so loops that
arrive in `hook()` by inlining (memset/bcmp loops) get guards like any
other loop.
Correctness of the inliner is tested differentially: fixture modules are
executed pre- and post-flatten in a wasm interpreter (dev-dependency) with
recorded host stubs, asserting identical results and host-call sequences —
an inlining bug must fail tests, not silently change hook semantics.

### 6.3 Guard pass (guard-checker equivalent + auto-insert)

Skipped entirely when `--api-version 1`.

For every function body, scan instructions; at each `loop` opcode:

- If the body already starts with `i32.const a; i32.const b; call $_g`
  (optionally followed by `drop`) — accept it, record `(a, b)`.
- Otherwise it is a **hard error by default**, reported with function index
  and instruction offset (pure guard-checker behavior, same as `check`).
  Developers fix it with `guard!` at the top of the loop body.
- With opt-in `--auto-guard`, the missing guard is instead inserted:
  `i32.const <id>; i32.const <maxiter>; call $_g; drop` immediately after
  the `loop` blocktype, id = `(1 << 30) + n` (sequential — disjoint from
  the `(1 << 31) + …` space used by `guard!`/`guard_m!`), maxiter =
  `--default-maxiter` (default 16, deliberately small). Auto-guard exists
  primarily for **compiler-generated loops** the developer never wrote —
  `compiler_builtins` `memcpy`/`memset` loops are the known offenders on
  `wasm32v1-none` (no bulk-memory ⇒ byte loops). Whether examples can stay
  guard-clean without it is validated empirically in phase 4; if they
  cannot, revisit the default with that evidence.

Rationale for default-off (review finding): silent insertion with a small
maxiter can turn into runtime `GUARD_VIOLATION`s, and it hides the real
worst-case instruction budget that SetHook fee estimation is based on.

**Phase-4 empirical results** (2026-07-23, confirming both sides of this
trade-off):
- Compiler-generated loops are real. `firewall`'s `[u8; 20]` equality
  lowers to a bcmp-style byte-compare loop; `emit-txn`'s 320-byte buffer
  zero-init lowers to a `compiler_builtins`-style memset function with 5
  loop constructs. Neither has any loop in Rust source; both need
  `--auto-guard`.
- Straight-line hooks (`accept-all`) and hooks whose only loops are
  source-level with `guard!` (`state-counter`) build clean with no flags —
  the strict default is workable.
- **The `--default-maxiter` CLI default must not be trusted for real
  deployments**: hooks-build validates guard *shape*, not that maxiter
  covers the loop's true runtime bound. maxiter 16 passes `check` for both
  examples above yet would raise `GUARD_VIOLATION` on-ledger (the compare
  loop runs up to 20 iterations; the memset bulk loop ~40). The examples
  size maxiter from disassembly (24 and 48 respectively) and the CLI docs
  must tell users to do the same.
- **Post-vendoring correction**: the four phase-4 artifacts, though accepted
  by the Rust reimplementation, are all rejected by the real (vendored)
  checker — see 6.2b R1/R2. The guard findings above remain valid (the
  loops still exist and still need guards), but compiler-generated loops
  additionally must be *inlined into the entry function* (flatten pass);
  their guards are then inserted at the inlined loop heads.
- **Static-buffer idiom** (added after the `emit-txn` rework): constant
  templates belong in `static`s (⇒ data segment, not a runtime chain of
  store instructions) and large zero-initialized buffers in zero-init
  `static`s (⇒ BSS: no data bytes, no code, and **no compiler-generated
  memset loop at all**). Exclusivity is sound — hooks are single-threaded
  and each invocation gets a fresh instance — and is packaged safely as
  `hooks_lib::static_cell::HookStatic<T>` (take-once cell: `take()` yields
  the one `&'static mut`, second call returns `None`; the only `unsafe`
  lives inside hooks-lib, and hook code needs no `unsafe` and no clippy
  allows). This removed emit-txn's memset entirely: no `--auto-guard`,
  WCE 6798 → ~350 and ~1.5 KB total (current-toolchain measurements —
  exact figures drift with compiler versions; `hooks-build build` prints
  the authoritative numbers. The take-once flag costs a few dozen bytes
  over a raw `static mut`, which in turn required
  `unsafe { &mut *&raw mut }` plus a `clippy::deref_addrof` allow at
  every site). Source-level avoidance of
  *initialization* libcalls is thus reliable via statics; comparison
  libcalls (bcmp from `[u8; N]` `==`) still need `--auto-guard` (see
  firewall).

If a guard was inserted and `_g` is not imported, the import is added
(import section rewrite ⇒ function index shift ⇒ handled by the same
renumbering machinery as GC).

**After any mutation, the full guard verifier and validator (6.4) run again
on the final bytes** — `build` never emits an artifact that `check` would
reject; a bug in the insertion pass fails the build instead of shipping.
For api-version 0 the authoritative final verdict comes from the vendored
upstream checker (6.5), not from the Rust reimplementation.

`emit()` reachability note: xahaud requires hooks that `emit` to have called
`etxn_reserve`; that is runtime behavior, not validated here.

### 6.4 Validator

Hard errors (module shape — the SetHook-derived rule set; the final
authority is `SetHook.cpp`, and phase 3 includes cross-checking every rule
against xahaud source plus a known-good C-built hook fixture):
- Any export other than `hook`/`cbak`; missing `hook`; wrong signatures.
- Any import from a module other than `env`, or a function name outside the
  whitelist (the whitelist is generated from `extern.h` — single source of
  truth shared with hooks-core; kept as a checked-in table with a test that
  it matches hooks-core's extern block). Import signature mismatch against
  `extern.h` types. Imported memories, tables, or globals.
- A `start` section.
- Passive data/element segments, `data count` section, or any element
  segment form beyond MVP active-with-function-indices.
- More than one (or zero, when data segments exist) defined memory;
  memory initial size beyond xahaud limits.
- Any floating-point opcode; any post-MVP opcode (encoding-level check via
  `wasmparser` configured to MVP-only features).
- `call_indirect` (v1 hard error: it defeats the recursion check and
  reachability analysis; revisit only with conservative table analysis).
- Call-graph cycle (recursion) — DFS over direct calls (C5); sound because
  `call_indirect` is banned.
- For api-version 0: any unguarded `loop` (both `check` mode and the
  post-mutation re-verification in `build`).
- For api-version 0: missing `_g` import (6.2b R1), and any type-section
  entry that is not an import's type or the entry-point type (6.2b R2).
- For api-version 0: block nesting depth > 32 (`Guard.h` `NESTING_LIMIT`
  under `GuardRuleDepth32`; warning from depth 28 — see 6.2c).
- Binary > 65,535 bytes. (`build` refuses to emit; `--allow-oversize`
  writes the artifact anyway for size-debugging, clearly marked INVALID.)

Warnings:
- Mutable defined globals beyond the shadow stack pointer pattern (allowed,
  but flagged as a size/audit smell).
- Size approaching the limit (≥ 56 KiB) — printed with the fee estimate.

Validation always runs against the **final output bytes** (post-clean,
post-guard), and `check <file>` applies the identical rule set to arbitrary
external wasm (including C-built hooks).

### 6.5 Verdict authority: the vendored upstream checker

The final accept/reject verdict for API-version-0 modules comes from
**xahaud's own guard checker, compiled into hooks-build from vendored,
byte-identical upstream source** — not from a Rust reimplementation. A port,
however careful, can diverge from what the node actually runs; the checker
is consensus logic, not a reference tool, so divergence means "hooks-build
says valid, SetHook says `temMALFORMED`" (or worse, vice versa).

Vendored files (upstream `Xahau/xahaud`, branch `release`, kept verbatim —
never hand-edited; re-sync only via `scripts/sync-vendor.sh`, which also
regenerates the `SHA256SUMS` tripwire file; CI verifies byte-identity
against upstream on every push/PR and weekly —
`.github/workflows/vendor-sync.yml`):

- `include/xrpl/hook/Guard.h` — `validateGuards()` / `check_guard()`
- `include/xrpl/hook/Enum.h` — log codes, `APIWhitelist`,
  `getImportWhitelist()`, guard-rules versioning
- `include/xrpl/hook/hook_api.macro` — the API table behind the whitelist

Upstream explicitly supports standalone compilation via
`-DGUARD_CHECKER_BUILD` (Enum.h stubs `uint256`/`Rules` with an
"all amendments enabled" `Rules`, which also yields the current
`getGuardRulesVersion` bit set). A small `guard_shim.cpp` (the only C++ we
author) exposes one `extern "C"` entry point: bytes in → verdict, the
upstream log text (captured from `GuardLog`), and on success the
worst-case instruction counts for `hook()`/`cbak()` that `validateGuards`
computes. Built by `build.rs` via the `cc` crate (C++17); a host C++
compiler becomes a build requirement of hooks-build.

`validateGuards` covers far more than loop guards (imports vs whitelist,
export shape, `call_indirect`, memory limits, custom sections, instruction
legality), so the division of labor is:

- **C++ vendored checker** — authoritative pass/fail for api-version 0, in
  both `check` and post-transform `build`. Its captured log is printed on
  failure verbatim; on success the instruction counts are reported (they
  are also what SetHook fee estimation derives from). Note: these are
  *syntactic* worst-case counts (a host `call` counts as 1; host-function
  work is not modeled), and the node's live `HookInstructionCount` meter
  can exceed them for tiny functions — observed live: emit-txn's `cbak`
  10 vs static 7 (see docs/E2E-TESTING.md). They are a fee-estimation
  input, not a runtime ceiling.
- **Rust pipeline (6.2–6.4)** — everything the checker does not do
  (cleaning, auto-guard insertion, the 65,535-byte size gate, fee
  estimate, api-version 1 checks) plus pre-transform diagnostics with
  precise function/offset locations, which upstream's log lacks. If the
  Rust validator and the C++ checker ever disagree, the C++ verdict wins
  and the disagreement is surfaced as a hooks-build bug.

The cleaner remains native Rust (upstream hook-cleaner is a separate
project, and cleaning is a transform whose output the authoritative checker
then judges); `wasmparser`/`wasm-encoder` byte-exactness (C8) is unchanged.
Behavioral reference tests compare verdicts on known-good/known-bad
fixtures, including the built examples.

## 7. examples/

Own workspace; every crate:

```toml
[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

Directory names are numbered in suggested reading order (`01_`..`10_`);
package names themselves are not (Cargo package names can't start with a
digit) — see `examples/README.md`.

| # | example | demonstrates |
|---|---|---|
| 01 | `accept-all` | minimal hook: `accept` everything (starter template) |
| 02 | `state-counter` | `state`/`state_set` round-trip, counter in hook state |
| 03 | `hook-params` | `hook_param`-configurable threshold, with a compiled-in default |
| 04 | `errors` | a meaningful `hook_errors!`-based rollback error-code system |
| 05 | `firewall` | read `otxn_field(sfAccount)` + hook param blacklist → `rollback` |
| 06 | `guard-patterns` | `guard!`/`guard_m!` correctness and the array-`==` memcmp-loop pitfall |
| 07 | `xfl-math` | reading `Amount` as XFL, `mulratio`, `Result`-based comparisons |
| 08 | `slot-ledger` | transaction field access via the Slot API |
| 09 | `state-foreign` | `state_foreign`: reading another account's hook state |
| 10 | `emit-txn` | `etxn_reserve` + a user-declared `txn_template!` Payment + `cbak` |

Each README shows the exact build command:
`hooks-build build --manifest-path examples/02_state-counter/Cargo.toml`
(or via mise task `mise run build-examples`, which builds all examples and
`check`s the outputs — this doubles as the end-to-end test).

Source style rules for examples (enforced by review, documented in
`examples/README.md`): no slice indexing that can panic (use fixed-size
arrays and `split_at`-free patterns), no `format!`/`fmt`, loops carry
`guard!` when the bound is known, and constant templates / large
zero-initialized buffers live in `static`s (data segment / BSS) rather
than stack locals (see §6.3's static-buffer idiom).

## 8. Code health

- `rustfmt.toml` (defaults; `edition = "2024"`), formatting enforced.
- Workspace lints in root `Cargo.toml`, inherited via `[lints] workspace = true`:
  `rust.unsafe_op_in_unsafe_fn = "deny"`, `clippy.all = "warn"`,
  `clippy.pedantic` selectively, `rust.missing_docs = "deny"` for the two
  library crates.
- **Panic-free is enforced, not promised** (review finding): hooks-lib and
  every example crate additionally deny `clippy::unwrap_used`,
  `clippy::expect_used`, `clippy::panic`, `clippy::indexing_slicing`, and
  `clippy::arithmetic_side_effects` is at least `warn`. The documented
  contract: hooks-lib wrappers are panic-free; hook crates keep that
  property only by passing these lints (checked by `mise run lint`).
- `mise.toml` tasks: `fmt`, `lint` (clippy `-D warnings`, both workspaces,
  host + wasm32v1-none targets), `test`, `build-wasm`, `build-examples`.
  Target-specific caveats: `build-wasm` scopes to `-p hooks-core -p
  hooks-lib` (hooks-build is a std CLI and must not be built for
  wasm32v1-none), and clippy for the examples workspace uses `--lib`, not
  `--all-targets` — wasm32v1-none has no `test` crate, so the implicit
  test-profile target can never build (examples also set `[lib] test =
  false`).
- Tests: hooks-build unit tests on `wat`-authored fixtures (cleaner strips
  exports; guard inserted at loop head byte-exactly; recursion detected;
  float opcode rejected); hooks-core has a test asserting the whitelist
  table and extern block stay in sync; examples built+checked in
  `build-examples`.
- `.gitignore`: `/target`, `/examples/target`, `/examples/**/out`, `out/`,
  `*.wasm` outside fixtures, `.DS_Store`. Binary test fixtures live in
  `crates/hooks-build/tests/fixtures/` and are exempted.

## 9. Implementation plan (delegation map)

| phase | content | executor |
|---|---|---|
| 0 | scaffolding: workspace, toolchain, fmt/lint, mise, .gitignore | Sonnet |
| 1 | hooks-core (mechanical header translation; headers provided) | Sonnet |
| 2 | hooks-lib (error, types, api wrappers, XFL, macros) | Sonnet |
| 3 | hooks-build (CLI, cleaner, guard pass, validator, tests) | Sonnet (Opus if stuck on encoder subtleties) |
| 4 | examples + end-to-end build via hooks-build | Sonnet |
| — | design, per-phase spec, review gates, final integration | Fable |

Each phase lands only after `mise run fmt && mise run lint && mise run test`
pass and the phase output is reviewed against this document.

## 10. Resolved questions

Settled during the external design review (recommendations adopted):

1. **Guard auto-insertion: default OFF.** Missing guards are hard errors
   with precise locations; `--auto-guard` is opt-in (see 6.3, including the
   compiler-generated-loop caveat that keeps the flag alive).
2. **Module-shape validation broadened**: start section, passive segments,
   data-count, imported memories/tables/globals, element-segment forms,
   mutable globals, multiple memories are all explicitly ruled on (6.4).
3. **XFL stays without `PartialEq`/`PartialOrd`**; explicit `bits_eq` if
   representation equality is ever needed.
4. **`call_indirect` is a v1 hard error** (keeps recursion detection and
   reachability sound); table + element segments are dropped by the cleaner.
5. **`hooks-build new` deferred** — copying `examples/01_accept-all` is the
   v1 scaffold story.

## 11. Design review record

- Reviewer: Codex CLI, model **gpt-5.5**, reasoning effort high, read-only
  (gpt-5.6 was requested first but is not available via the Codex CLI).
- Verdict: rework — 12 findings (2 blockers, 7 major, 3 minor), all
  incorporated:
  size-limit hard error (6.4); `guard!` matches C `GUARD`/`GUARDM` ABI
  including `+1` (5.4); post-mutation re-validation in `build` (6.3);
  `call_indirect` hard error + sound recursion check (6.4); whole-module
  index remap specification (6.2); precise GC roots and table/element
  dropping (6.2); explicit module-shape rule set with xahaud cross-check
  task (6.4); clippy-enforced panic-freedom (8); non-panicking host stubs
  returning `NOT_IMPLEMENTED` (4); `macro.h`/`hookapi.h` constant scope
  (4); private XFL field with raw-bits accessors (5.3); pointer-direction
  discipline in wrappers (5.2).
