//! `keylets` — computes 25 of the 26 `KEYLET_*` types via
//! `hooks_lib::api::keylet`'s typed `keylet_xxx` helpers (one per
//! `hooks_core::consts::KEYLET_*` constant), and stores every result in
//! hook state so an e2e test can read them back and check each one against
//! an independently computed expected value.
//!
//! `KEYLET_TICKET` is the one exception: live testing found
//! `keylet_ticket` reliably fails against this exact node build regardless
//! of its arguments, while every structurally similar type succeeds — see
//! the README's "e2e verification scope" section and
//! `hooks_lib::api::keylet::keylet_ticket`'s own doc comment for the full
//! writeup. `KeyletKey::Ticket` stays declared below (so no other
//! variant's discriminant shifts) but [`my_hook`] never computes or stores
//! it.
//!
//! Every keylet actually computed is derived from a small set of fixed,
//! deterministic inputs — the invoking transaction's own
//! `sfAccount`/`sfDestination` (as `owner`/`dest`) plus a handful of
//! `const` test values below — so `e2e/test/keylets.test.ts` can reproduce
//! the exact same inputs and recompute the exact same 34-byte result
//! independently, with no dependency on any *other* ledger object actually
//! existing.
//!
//! Every result is written to hook state keyed by [`KeyletKey`], whose 26
//! unit variants are declared in the same order as their `KEYLET_*`
//! constant (`KeyletKey::Hook`'s discriminant `0` == `KEYLET_HOOK`'s value
//! `1`, minus one; ...; `KeyletKey::Cron`'s discriminant `25` == `KEYLET_CRON`'s
//! value `26`, minus one) — `e2e/test/keylets.test.ts` relies on this exact
//! mapping (discriminant = `KEYLET_*` value − 1) to read each entry back by
//! its own hand-built `state_keys!`-equivalent tag byte.
//!
//! Build: `hooks-build build --manifest-path examples/13_keylets/Cargo.toml --auto-guard --default-maxiter 34`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback, state_keys};

state_keys! {
    /// One state entry per `KEYLET_*` type. See the module doc comment for
    /// why declaration order here is load-bearing.
    enum KeyletKey {
        /// `KEYLET_HOOK` (1).
        Hook,
        /// `KEYLET_HOOK_STATE` (2).
        HookState,
        /// `KEYLET_ACCOUNT` (3).
        Account,
        /// `KEYLET_AMENDMENTS` (4).
        Amendments,
        /// `KEYLET_CHILD` (5).
        Child,
        /// `KEYLET_SKIP` (6).
        Skip,
        /// `KEYLET_FEES` (7).
        Fees,
        /// `KEYLET_NEGATIVE_UNL` (8).
        NegativeUnl,
        /// `KEYLET_LINE` (9).
        Line,
        /// `KEYLET_OFFER` (10).
        Offer,
        /// `KEYLET_QUALITY` (11).
        Quality,
        /// `KEYLET_EMITTED_DIR` (12).
        EmittedDir,
        /// `KEYLET_TICKET` (13).
        Ticket,
        /// `KEYLET_SIGNERS` (14).
        Signers,
        /// `KEYLET_CHECK` (15).
        Check,
        /// `KEYLET_DEPOSIT_PREAUTH` (16).
        DepositPreauth,
        /// `KEYLET_UNCHECKED` (17).
        Unchecked,
        /// `KEYLET_OWNER_DIR` (18).
        OwnerDir,
        /// `KEYLET_PAGE` (19).
        Page,
        /// `KEYLET_ESCROW` (20).
        Escrow,
        /// `KEYLET_PAYCHAN` (21).
        Paychan,
        /// `KEYLET_EMITTED` (22).
        Emitted,
        /// `KEYLET_NFT_OFFER` (23).
        NftOffer,
        /// `KEYLET_HOOK_DEFINITION` (24).
        HookDefinition,
        /// `KEYLET_HOOK_STATE_DIR` (25).
        HookStateDir,
        /// `KEYLET_CRON` (26).
        Cron,
    }
}

hook_errors! {
    /// `keylets` rollback codes.
    pub enum KeyletsError {
        /// The originating transaction has no `sfAccount` field (should be
        /// unreachable — every real transaction has one).
        AccountFieldMissing = 1,
        /// The originating transaction has no `sfDestination` field
        /// (unreachable for an `Invoke` addressed to this hook's account).
        DestinationFieldMissing = 2,
        /// Writing a computed keylet to hook state failed.
        StateWriteFailed = 4,
        // Codes 101..126 (100 + a `KEYLET_*` constant) are reserved for a
        // `keylet_xxx` compute failure identifying exactly which of the 26
        // types it was — see [`compute`]'s own doc comment. Not spelled as
        // variants here since they're generated, not fixed, codes.
    }
}

/// Fixed test hash — used for every `keylet_xxx` argument shaped like a
/// raw 32-byte hash ([`keylet_child`]/[`keylet_unchecked`]/
/// [`keylet_emitted`]/[`keylet_hook_definition`]). Arbitrary but
/// deterministic; `e2e/test/keylets.test.ts` uses the identical byte
/// pattern to recompute the expected value independently.
const TEST_HASH: Hash = Hash([0xAB; 32]);

/// Fixed test hook-state key — see [`keylet_hook_state`].
const TEST_STATE_KEY: StateKey = StateKey([0xCD; 32]);

/// Fixed test hook-state namespace — see [`keylet_hook_state`]/
/// [`keylet_hook_state_dir`].
const TEST_NAMESPACE: NameSpace = NameSpace([0xEF; 32]);

/// Fixed test currency code — `USD`, encoded the standard "3-letter ISO
/// code in bytes 12..15, zero elsewhere" way (see [`keylet_line`]).
const TEST_CURRENCY: CurrencyCode = CurrencyCode([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'U', b'S', b'D', 0, 0, 0, 0, 0,
]);

/// A placeholder order-book/owner directory keylet — only [`keylet_quality`]
/// uses this, and its result isn't independently re-verified in the e2e
/// suite (see the suite's own comment for why). Not all-zero, though: the
/// host validates that `dir`'s own 2-byte type prefix is a real
/// `ltDIR_NODE` (`0x0064`, ASCII `'d'` — the single ledger-entry type
/// shared by owner *and* order-book directories) before deriving a page
/// keylet from it — an all-zero (`type = 0`) placeholder fails that check
/// with `INVALID_ARGUMENT`.
const TEST_DIR: Keylet = Keylet([
    0x00, 0x64, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB,
    0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB, 0xAB,
    0xAB, 0xAB,
]);

const OFFER_SEQ: u32 = 1;
const ESCROW_SEQ: u32 = 2;
const CHECK_SEQ: u32 = 3;
// TICKET_SEQ intentionally unused - see the KeyletKey::Ticket comment in
// my_hook() below.
const PAYCHAN_SEQ: u32 = 5;
const NFT_OFFER_SEQ: u32 = 6;
/// `keylet_cron`'s second argument is a raw start-time value, not a
/// per-account sequence counter (confirmed against `xahau` npm's own
/// `hashCron` implementation — see the e2e suite's comment).
const CRON_START_TIME: u32 = 1_700_000_000;
const QUALITY_HIGH: u32 = 10;
const QUALITY_LOW: u32 = 20;
const PAGE_INDEX_HIGH: u32 = 1;
const PAGE_INDEX_LOW: u32 = 2;

/// Unwraps a `util_keylet` result, rolling back with `100 + keylet_type` on
/// failure (`101` for `KEYLET_HOOK`, ..., `126` for `KEYLET_CRON`) so a
/// failure identifies exactly which of the 26 types it was, instead of
/// collapsing every possible failure into one indistinguishable shared
/// code. This is exactly how the `KEYLET_TICKET` host limitation
/// documented in the module doc comment was originally found and
/// isolated.
#[inline(always)]
fn compute(keylet_type: u32, result: Result<Keylet>) -> Keylet {
    match result {
        Ok(k) => k,
        Err(_) => rollback!(
            b"keylets: a keylet_xxx call failed",
            100i64.wrapping_add(keylet_type as i64)
        ),
    }
}

/// Writes `value` to this hook's own state under `key`, rolling back with
/// [`KeyletsError::StateWriteFailed`] on failure.
///
/// Uses the raw `hooks_lib::api::state::state_set` directly (a raw byte
/// buffer, no size cap) rather than the typed `state_set_loose`/
/// `state_set_typed` layer — a [`Keylet`] is 34 bytes, over that layer's
/// 32-byte convenience cap (see `hooks_lib::state`'s module doc comment
/// for why, and which other `hooks_lib::types` newtypes hit the same
/// limit — `PublicKey` (33) and `IouAmount` (48) among them).
#[inline(always)]
fn store(key: &KeyletKey, value: &Keylet) {
    if state_set(value.as_ref(), &key.encode()).is_err() {
        rollback!(b"keylets: state_set failed", KeyletsError::StateWriteFailed);
    }
}

/// Hook entry point. See the module doc comment for the full behavior.
#[hook]
fn my_hook() -> i64 {
    let owner: AccountId = match otxn_field_exact(sfAccount) {
        Ok(v) => v,
        Err(_) => rollback!(
            b"keylets: sfAccount missing from the originating transaction",
            KeyletsError::AccountFieldMissing
        ),
    };
    let dest: AccountId = match otxn_field_exact(sfDestination) {
        Ok(v) => v,
        Err(_) => rollback!(
            b"keylets: sfDestination missing from the originating transaction",
            KeyletsError::DestinationFieldMissing
        ),
    };

    store(&KeyletKey::Hook, &compute(KEYLET_HOOK, keylet_hook(&owner)));
    store(
        &KeyletKey::HookState,
        &compute(
            KEYLET_HOOK_STATE,
            keylet_hook_state(&owner, &TEST_STATE_KEY, &TEST_NAMESPACE),
        ),
    );
    store(
        &KeyletKey::Account,
        &compute(KEYLET_ACCOUNT, keylet_account(&owner)),
    );
    store(
        &KeyletKey::Amendments,
        &compute(KEYLET_AMENDMENTS, keylet_amendments()),
    );
    store(
        &KeyletKey::Child,
        &compute(KEYLET_CHILD, keylet_child(&TEST_HASH)),
    );
    store(&KeyletKey::Skip, &compute(KEYLET_SKIP, keylet_skip(None)));
    store(&KeyletKey::Fees, &compute(KEYLET_FEES, keylet_fees()));
    store(
        &KeyletKey::NegativeUnl,
        &compute(KEYLET_NEGATIVE_UNL, keylet_negative_unl()),
    );
    store(
        &KeyletKey::Line,
        &compute(KEYLET_LINE, keylet_line(&owner, &dest, &TEST_CURRENCY)),
    );
    store(
        &KeyletKey::Offer,
        &compute(KEYLET_OFFER, keylet_offer(&owner, OFFER_SEQ)),
    );
    store(
        &KeyletKey::Quality,
        &compute(
            KEYLET_QUALITY,
            keylet_quality(&TEST_DIR, QUALITY_HIGH, QUALITY_LOW),
        ),
    );
    store(
        &KeyletKey::EmittedDir,
        &compute(KEYLET_EMITTED_DIR, keylet_emitted_dir()),
    );
    // KeyletKey::Ticket is deliberately NOT computed/stored here - see
    // `keylet_ticket`'s own doc comment ("Known host limitation") and this
    // crate's README's "e2e verification scope" section. The `Ticket`
    // variant stays declared (so every other variant's `state_keys!`
    // discriminant is unaffected) but its state entry is simply never
    // written.
    store(
        &KeyletKey::Signers,
        &compute(KEYLET_SIGNERS, keylet_signers(&owner)),
    );
    store(
        &KeyletKey::Check,
        &compute(KEYLET_CHECK, keylet_check(&owner, CHECK_SEQ)),
    );
    store(
        &KeyletKey::DepositPreauth,
        &compute(
            KEYLET_DEPOSIT_PREAUTH,
            keylet_deposit_preauth(&owner, &dest),
        ),
    );
    store(
        &KeyletKey::Unchecked,
        &compute(KEYLET_UNCHECKED, keylet_unchecked(&TEST_HASH)),
    );
    store(
        &KeyletKey::OwnerDir,
        &compute(KEYLET_OWNER_DIR, keylet_owner_dir(&owner)),
    );
    store(
        &KeyletKey::Page,
        &compute(
            KEYLET_PAGE,
            keylet_page(&TEST_HASH, PAGE_INDEX_HIGH, PAGE_INDEX_LOW),
        ),
    );
    store(
        &KeyletKey::Escrow,
        &compute(KEYLET_ESCROW, keylet_escrow(&owner, ESCROW_SEQ)),
    );
    store(
        &KeyletKey::Paychan,
        &compute(KEYLET_PAYCHAN, keylet_paychan(&owner, &dest, PAYCHAN_SEQ)),
    );
    store(
        &KeyletKey::Emitted,
        &compute(KEYLET_EMITTED, keylet_emitted(&TEST_HASH)),
    );
    store(
        &KeyletKey::NftOffer,
        &compute(KEYLET_NFT_OFFER, keylet_nft_offer(&owner, NFT_OFFER_SEQ)),
    );
    store(
        &KeyletKey::HookDefinition,
        &compute(KEYLET_HOOK_DEFINITION, keylet_hook_definition(&TEST_HASH)),
    );
    store(
        &KeyletKey::HookStateDir,
        &compute(
            KEYLET_HOOK_STATE_DIR,
            keylet_hook_state_dir(&owner, &TEST_NAMESPACE),
        ),
    );
    store(
        &KeyletKey::Cron,
        &compute(KEYLET_CRON, keylet_cron(&owner, CRON_START_TIME)),
    );

    accept!(b"keylets: ok", 0)
}
