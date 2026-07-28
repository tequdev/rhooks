//! One typed helper per [`hooks_core::consts`] `KEYLET_*` constant, built on
//! top of [`crate::api::util::util_keylet_buf`] — the untyped, one-function-
//! for-every-type escape hatch that takes `keylet_type` and up to six raw
//! `u32` components (`a`..`f`) and stays available for anything not covered
//! below (or a future protocol keylet type this crate hasn't caught up
//! with yet).
//!
//! # Why typed helpers, and why one per type
//!
//! [`util_keylet`](crate::api::util::util_keylet)/[`util_keylet_buf`] take
//! `a`..`f` as bare `u32`s — some are raw values (a sequence number, a
//! quality component), others are **pointers** into this hook's own linear
//! memory (an account ID, a hash, a currency code), and *which* is which,
//! how many of the six are actually used, and what they mean all depend
//! silently on `keylet_type`. Nothing at the type level stops passing an
//! account pointer where a sequence number was expected, or omitting a
//! component a given type actually requires — a mismatch is either a
//! `NO_SUCH_KEYLET`/`INVALID_ARGUMENT` at runtime or, worse, a keylet that
//! silently resolves to the wrong ledger entry.
//!
//! Every function below instead takes exactly the fixed-size
//! `hooks_lib::types` newtype(s) and/or plain integer(s) its own keylet
//! type actually needs — [`keylet_account`] takes an `&AccountId` and
//! nothing else, [`keylet_line`] takes two `&AccountId`s and a
//! `&CurrencyCode`, [`keylet_offer`] takes an `&AccountId` and a `u32`
//! sequence — encoding each type's own argument shape as its function
//! signature instead of six same-typed slots meaning something different
//! per call. Every one is a thin, `#[inline(always)]` pass-through to
//! [`util_keylet_buf`] (computing each pointer/length pair via `.as_ptr()`/
//! `.len()` on the newtype argument, `0` for every unused `a`..`f` slot),
//! so none of this costs anything beyond the raw host call itself — see
//! [`util_keylet_buf`]'s own doc comment for a toolchain note every caller
//! of *any* function in this module needs (a 34-byte `Keylet` scratch
//! buffer needs `--auto-guard --default-maxiter 34` to build past the
//! guard checker).
//!
//! # Source of truth
//!
//! Every `KEYLET_*` constant this module covers comes from
//! [`hooks_core::consts`] (itself generated from the vendored
//! `hook/hookapi.h` — see `hooks-core`'s own module doc comment), and every
//! function below is named `keylet_xxx` for the constant `KEYLET_XXX` it
//! wraps — `keylet_account` for `KEYLET_ACCOUNT`, `keylet_hook_state` for
//! `KEYLET_HOOK_STATE`, and so on, with the one deliberate exception that
//! the constant historically named `KEYLET_EMITTED` (not
//! `KEYLET_EMITTED_TXN`) backs [`keylet_emitted`] — kept aligned with the
//! constant's actual name rather than a more descriptive alternative, so
//! the `KEYLET_*` constant and its typed helper are always a mechanical,
//! one-to-one lookup.

use crate::api::util::util_keylet_buf;
use crate::error::Result;
use crate::types::{AccountId, CurrencyCode, Hash, Keylet, NameSpace, StateKey};
use hooks_core::consts::{
    KEYLET_ACCOUNT, KEYLET_AMENDMENTS, KEYLET_CHECK, KEYLET_CHILD, KEYLET_CRON,
    KEYLET_DEPOSIT_PREAUTH, KEYLET_EMITTED, KEYLET_EMITTED_DIR, KEYLET_ESCROW, KEYLET_FEES,
    KEYLET_HOOK, KEYLET_HOOK_DEFINITION, KEYLET_HOOK_STATE, KEYLET_HOOK_STATE_DIR, KEYLET_LINE,
    KEYLET_NEGATIVE_UNL, KEYLET_NFT_OFFER, KEYLET_OFFER, KEYLET_OWNER_DIR, KEYLET_PAGE,
    KEYLET_PAYCHAN, KEYLET_QUALITY, KEYLET_SIGNERS, KEYLET_SKIP, KEYLET_TICKET, KEYLET_UNCHECKED,
};

/// `KEYLET_HOOK` (1): the keylet for `account`'s installed `Hook` ledger
/// object (the object holding that account's chain of hooks — distinct
/// from [`keylet_hook_definition`], which keys a single hook's own,
/// account-independent definition object).
#[inline(always)]
pub fn keylet_hook(account: &AccountId) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_HOOK,
        account.as_ptr() as u32,
        account.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_HOOK_STATE` (2): the keylet for one hook-state entry —
/// `account`'s state keyed by `key`, inside `namespace`. This is an
/// alternate route to the same state entry [`crate::state`]'s
/// `state_get`/`state_set_loose` (+ `_foreign` twins) read/write directly
/// by key; reach for this when a keylet (rather than a decoded value) is
/// what's actually needed — e.g. to pass to [`crate::api::slot::slot_set`]
/// or another Hook API that takes a keylet.
#[inline(always)]
pub fn keylet_hook_state(
    account: &AccountId,
    key: &StateKey,
    namespace: &NameSpace,
) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_HOOK_STATE,
        account.as_ptr() as u32,
        account.len() as u32,
        key.as_ptr() as u32,
        key.len() as u32,
        namespace.as_ptr() as u32,
        namespace.len() as u32,
    )
}

/// `KEYLET_ACCOUNT` (3): the keylet for `account`'s own `AccountRoot`
/// ledger object.
#[inline(always)]
pub fn keylet_account(account: &AccountId) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_ACCOUNT,
        account.as_ptr() as u32,
        account.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_AMENDMENTS` (4): the keylet for the ledger's singleton
/// `Amendments` object. Takes no arguments — every component the host
/// call itself takes must be `0`.
#[inline(always)]
pub fn keylet_amendments() -> Result<Keylet> {
    util_keylet_buf(KEYLET_AMENDMENTS, 0, 0, 0, 0, 0, 0)
}

/// `KEYLET_CHILD` (5): a keylet derived from `parent`, one level down —
/// the same "hash a parent index to get a pseudo-account's own index"
/// pattern the protocol uses internally for a handful of derived ledger
/// objects.
#[inline(always)]
pub fn keylet_child(parent: &Hash) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_CHILD,
        parent.as_ptr() as u32,
        parent.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_SKIP` (6): the keylet for a `SkipList` ledger object.
/// `ledger_index`: `None` for the current skip list (the common case, at
/// its fixed well-known index); `Some(seq)` for the skip list as of a
/// specific historical ledger sequence.
#[inline(always)]
pub fn keylet_skip(ledger_index: Option<u32>) -> Result<Keylet> {
    match ledger_index {
        Some(seq) => util_keylet_buf(KEYLET_SKIP, seq, 1, 0, 0, 0, 0),
        None => util_keylet_buf(KEYLET_SKIP, 0, 0, 0, 0, 0, 0),
    }
}

/// `KEYLET_FEES` (7): the keylet for the ledger's singleton `FeeSettings`
/// object. Takes no arguments — every component the host call itself
/// takes must be `0`.
#[inline(always)]
pub fn keylet_fees() -> Result<Keylet> {
    util_keylet_buf(KEYLET_FEES, 0, 0, 0, 0, 0, 0)
}

/// `KEYLET_NEGATIVE_UNL` (8): the keylet for the ledger's singleton
/// `NegativeUNL` object. Takes no arguments — every component the host
/// call itself takes must be `0`.
#[inline(always)]
pub fn keylet_negative_unl() -> Result<Keylet> {
    util_keylet_buf(KEYLET_NEGATIVE_UNL, 0, 0, 0, 0, 0, 0)
}

/// `KEYLET_LINE` (9): the keylet for the trust line (`RippleState` ledger
/// object) between `account_a` and `account_b` in `currency` — order of
/// `account_a`/`account_b` does not matter, a trust line has no fixed
/// "side" (the protocol canonicalizes the two accounts internally when
/// computing the index).
#[inline(always)]
pub fn keylet_line(
    account_a: &AccountId,
    account_b: &AccountId,
    currency: &CurrencyCode,
) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_LINE,
        account_a.as_ptr() as u32,
        account_a.len() as u32,
        account_b.as_ptr() as u32,
        account_b.len() as u32,
        currency.as_ptr() as u32,
        currency.len() as u32,
    )
}

/// `KEYLET_OFFER` (10): the keylet for `account`'s `Offer` ledger object
/// created by the transaction at sequence `seq` (an `OfferCreate`'s own
/// `Sequence`, or the ticket sequence that authorized it).
#[inline(always)]
pub fn keylet_offer(account: &AccountId, seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_OFFER,
        account.as_ptr() as u32,
        account.len() as u32,
        seq,
        0,
        0,
        0,
    )
}

/// `KEYLET_QUALITY` (11): the keylet for the order book directory page at
/// exchange rate `quality_high`/`quality_low` (the top and bottom 32 bits
/// of the 64-bit quality value), rooted at the order-book directory `dir`.
#[inline(always)]
pub fn keylet_quality(dir: &Keylet, quality_high: u32, quality_low: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_QUALITY,
        dir.as_ptr() as u32,
        dir.len() as u32,
        quality_high,
        quality_low,
        0,
        0,
    )
}

/// `KEYLET_EMITTED_DIR` (12): the keylet for the ledger's singleton
/// directory of currently-outstanding emitted transactions. Takes no
/// arguments — every component the host call itself takes must be `0`.
#[inline(always)]
pub fn keylet_emitted_dir() -> Result<Keylet> {
    util_keylet_buf(KEYLET_EMITTED_DIR, 0, 0, 0, 0, 0, 0)
}

/// `KEYLET_TICKET` (13): the keylet for `account`'s `Ticket` ledger object
/// at ticket sequence `ticket_seq`.
///
/// # Known host limitation
///
/// Live e2e testing (`examples/13_keylets`, standalone `xahaud
/// 2026.6.21-release+3350`) found this specific call reliably fails at
/// runtime — `util_keylet` returns an error for `KEYLET_TICKET` regardless
/// of `ticket_seq`'s value, even though the identical `account`/
/// `ticket_seq` shape is accepted by that same node's `ledger_entry` RPC
/// (which computes the same index via a different code path) and every
/// structurally similar type (`KEYLET_OFFER`/`KEYLET_ESCROW`/
/// `KEYLET_CHECK`/`KEYLET_SIGNERS`, isolated the same way) succeeds. This
/// looks like a host-side gap in that specific `util_keylet`
/// implementation, not a bug in this wrapper's argument marshaling — kept
/// here (rather than removed) since the wrapper itself is correct per the
/// documented argument shape, and a future/different host build may
/// support it. `examples/13_keylets` does not exercise this call for that
/// reason — see its README's "e2e verification scope" section.
#[inline(always)]
pub fn keylet_ticket(account: &AccountId, ticket_seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_TICKET,
        account.as_ptr() as u32,
        account.len() as u32,
        ticket_seq,
        0,
        0,
        0,
    )
}

/// `KEYLET_SIGNERS` (14): the keylet for `account`'s `SignerList` ledger
/// object.
#[inline(always)]
pub fn keylet_signers(account: &AccountId) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_SIGNERS,
        account.as_ptr() as u32,
        account.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_CHECK` (15): the keylet for `account`'s `Check` ledger object
/// created by the transaction at sequence `seq`.
#[inline(always)]
pub fn keylet_check(account: &AccountId, seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_CHECK,
        account.as_ptr() as u32,
        account.len() as u32,
        seq,
        0,
        0,
        0,
    )
}

/// `KEYLET_DEPOSIT_PREAUTH` (16): the keylet for the `DepositPreauth`
/// ledger object recording that `owner` has preauthorized `authorized`.
#[inline(always)]
pub fn keylet_deposit_preauth(owner: &AccountId, authorized: &AccountId) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_DEPOSIT_PREAUTH,
        owner.as_ptr() as u32,
        owner.len() as u32,
        authorized.as_ptr() as u32,
        authorized.len() as u32,
        0,
        0,
    )
}

/// `KEYLET_UNCHECKED` (17): `hash` itself, reinterpreted directly as a
/// keylet index with no type-prefix validation — an escape hatch for a
/// ledger index already known to be correct (e.g. one read back from
/// another ledger object's own fields), not a *computed* keylet.
#[inline(always)]
pub fn keylet_unchecked(hash: &Hash) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_UNCHECKED,
        hash.as_ptr() as u32,
        hash.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_OWNER_DIR` (18): the keylet for `account`'s owner directory
/// (the root page listing every ledger object `account` owns).
#[inline(always)]
pub fn keylet_owner_dir(account: &AccountId) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_OWNER_DIR,
        account.as_ptr() as u32,
        account.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_PAGE` (19): the keylet for directory page
/// `index_high`/`index_low` (the top and bottom 32 bits of the page
/// index) of the directory rooted at `root` (that root directory's own
/// 32-byte ledger index — see [`keylet_owner_dir`]/[`keylet_quality`] for
/// how to obtain one).
#[inline(always)]
pub fn keylet_page(root: &Hash, index_high: u32, index_low: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_PAGE,
        root.as_ptr() as u32,
        root.len() as u32,
        index_high,
        index_low,
        0,
        0,
    )
}

/// `KEYLET_ESCROW` (20): the keylet for `account`'s `Escrow` ledger object
/// created by the transaction at sequence `seq`.
#[inline(always)]
pub fn keylet_escrow(account: &AccountId, seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_ESCROW,
        account.as_ptr() as u32,
        account.len() as u32,
        seq,
        0,
        0,
        0,
    )
}

/// `KEYLET_PAYCHAN` (21): the keylet for the `PayChannel` ledger object
/// from `src` to `dst` created by the transaction at sequence `seq`.
#[inline(always)]
pub fn keylet_paychan(src: &AccountId, dst: &AccountId, seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_PAYCHAN,
        src.as_ptr() as u32,
        src.len() as u32,
        dst.as_ptr() as u32,
        dst.len() as u32,
        seq,
        0,
    )
}

/// `KEYLET_EMITTED` (22): the keylet for the `EmittedTxn` bookkeeping
/// object tracking the previously-emitted transaction identified by
/// `hash`. Named for the constant it wraps (`hooks_core::consts::
/// KEYLET_EMITTED`, not `KEYLET_EMITTED_TXN`) — see this module's doc
/// comment.
#[inline(always)]
pub fn keylet_emitted(hash: &Hash) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_EMITTED,
        hash.as_ptr() as u32,
        hash.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_NFT_OFFER` (23): the keylet for `account`'s `NFTokenOffer`
/// ledger object created by the transaction at sequence `seq`.
#[inline(always)]
pub fn keylet_nft_offer(account: &AccountId, seq: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_NFT_OFFER,
        account.as_ptr() as u32,
        account.len() as u32,
        seq,
        0,
        0,
        0,
    )
}

/// `KEYLET_HOOK_DEFINITION` (24): the keylet for the account-independent
/// `HookDefinition` ledger object identified by `hash` (a hook's own wasm
/// hash, the same value `SetHook`'s `sfHookHash`/`hook_hash` names) —
/// distinct from [`keylet_hook`], which keys a specific *account's*
/// installed hook chain.
#[inline(always)]
pub fn keylet_hook_definition(hash: &Hash) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_HOOK_DEFINITION,
        hash.as_ptr() as u32,
        hash.len() as u32,
        0,
        0,
        0,
        0,
    )
}

/// `KEYLET_HOOK_STATE_DIR` (25): the keylet for the directory listing
/// every hook-state entry `account` has stored under `namespace`.
#[inline(always)]
pub fn keylet_hook_state_dir(account: &AccountId, namespace: &NameSpace) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_HOOK_STATE_DIR,
        account.as_ptr() as u32,
        account.len() as u32,
        namespace.as_ptr() as u32,
        namespace.len() as u32,
        0,
        0,
    )
}

/// `KEYLET_CRON` (26): the keylet for `account`'s `Cron` ledger object
/// starting at `start_time` (a raw ledger-time value — a `Cron` entry is
/// indexed by *when* it next fires, not by a per-account sequence
/// counter, unlike every other `account`-keyed type above).
#[inline(always)]
pub fn keylet_cron(account: &AccountId, start_time: u32) -> Result<Keylet> {
    util_keylet_buf(
        KEYLET_CRON,
        account.as_ptr() as u32,
        account.len() as u32,
        start_time,
        0,
        0,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        let account = AccountId::zeroed();
        let account_b = AccountId::zeroed();
        let hash = Hash::zeroed();
        let key = StateKey::zeroed();
        let namespace = NameSpace::zeroed();
        let currency = CurrencyCode::zeroed();
        let dir = Keylet::zeroed();

        assert_eq!(keylet_hook(&account), Err(HookError::NotImplemented));
        assert_eq!(
            keylet_hook_state(&account, &key, &namespace),
            Err(HookError::NotImplemented)
        );
        assert_eq!(keylet_account(&account), Err(HookError::NotImplemented));
        assert_eq!(keylet_amendments(), Err(HookError::NotImplemented));
        assert_eq!(keylet_child(&hash), Err(HookError::NotImplemented));
        assert_eq!(keylet_skip(None), Err(HookError::NotImplemented));
        assert_eq!(keylet_skip(Some(1)), Err(HookError::NotImplemented));
        assert_eq!(keylet_fees(), Err(HookError::NotImplemented));
        assert_eq!(keylet_negative_unl(), Err(HookError::NotImplemented));
        assert_eq!(
            keylet_line(&account, &account_b, &currency),
            Err(HookError::NotImplemented)
        );
        assert_eq!(keylet_offer(&account, 1), Err(HookError::NotImplemented));
        assert_eq!(keylet_quality(&dir, 1, 1), Err(HookError::NotImplemented));
        assert_eq!(keylet_emitted_dir(), Err(HookError::NotImplemented));
        assert_eq!(keylet_ticket(&account, 1), Err(HookError::NotImplemented));
        assert_eq!(keylet_signers(&account), Err(HookError::NotImplemented));
        assert_eq!(keylet_check(&account, 1), Err(HookError::NotImplemented));
        assert_eq!(
            keylet_deposit_preauth(&account, &account_b),
            Err(HookError::NotImplemented)
        );
        assert_eq!(keylet_unchecked(&hash), Err(HookError::NotImplemented));
        assert_eq!(keylet_owner_dir(&account), Err(HookError::NotImplemented));
        assert_eq!(keylet_page(&hash, 1, 1), Err(HookError::NotImplemented));
        assert_eq!(keylet_escrow(&account, 1), Err(HookError::NotImplemented));
        assert_eq!(
            keylet_paychan(&account, &account_b, 1),
            Err(HookError::NotImplemented)
        );
        assert_eq!(keylet_emitted(&hash), Err(HookError::NotImplemented));
        assert_eq!(
            keylet_nft_offer(&account, 1),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            keylet_hook_definition(&hash),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            keylet_hook_state_dir(&account, &namespace),
            Err(HookError::NotImplemented)
        );
        assert_eq!(keylet_cron(&account, 1), Err(HookError::NotImplemented));
    }
}
