//! Integration coverage for the typed slot layer: `SlotObject<T>`,
//! `SField<T>`, navigation gating, the read table, and `slot_path!`.
//!
//! An integration test (`tests/`), not an in-crate module, because the
//! generated `sfield` table and the prelude re-exports are part of what is
//! being checked and both are best exercised the way a hook crate sees them.
//!
//! # What a host build can prove here
//!
//! Every Hook API call resolves to `hooks-core`'s host stub on a non-wasm
//! target, and every stub returns `NOT_IMPLEMENTED`. So these tests prove
//! **typing, inference and reachability**: that the field constants carry the
//! right value types, that navigation is gated by parent type, that every
//! read exists with the signature the design specifies, and that the widened
//! field-code APIs accept typed, raw and stored `u32` alike.
//!
//! They cannot prove host *behavior* — which slot the host assigned, that a
//! parent can be cleared after deriving a child, that `take_*` really frees
//! a slot. Those are pinned live in `e2e/test/slot-ledger.test.ts`.
use hooks_lib::prelude::*;
use hooks_lib::slot_path;
use hooks_lib::types::{Amount, Issue, Opaque};

const STUB: HookError = HookError::NotImplemented;

#[test]
fn surface() {
    // constructors
    assert_eq!(SlotObject::from_otxn().err(), Some(STUB));
    assert_eq!(SlotObject::from_meta().err(), Some(STUB));
    let k = Keylet::default();
    assert_eq!(SlotObject::from_keylet(&k).err(), Some(STUB));
    let h = Hash::default();
    assert_eq!(SlotObject::from_txn_hash(&h).err(), Some(STUB));

    // typed field constants
    let _: SField<u32> = sfSequence;
    let _: SField<AccountId> = sfAccount;
    let _: SField<Amount> = sfBalance;
    let _: SField<STArray> = sfSignerEntries;
    let _: SField<Hash> = sfLedgerHash;
    let _: SField<u64> = sfExchangeRate;
    let _: SField<u8> = sfCloseResolution;
    let _: SField<u16> = sfTransactionType;
    let _: SField<STObject> = sfMemo;
    let _: SField<Issue> = sfAsset;
    // Blob / Hash160 / PathSet -> Opaque
    let _: SField<Opaque> = sfSigningPubKey;
    let _: SField<Opaque> = sfTakerPaysCurrency;

    // code() const bridge + parity
    const SEQ: u32 = sfSequence.code();
    assert_eq!(SEQ, hooks_lib::raw::sfcodes::sfSequence);
    let widened: u32 = sfAccount.into();
    assert_eq!(widened, hooks_lib::raw::sfcodes::sfAccount);

    // widened APIs accept typed, raw, and stored u32
    let mut buf = [0u8; 20];
    assert_eq!(otxn_field(&mut buf, sfAccount).err(), Some(STUB));
    assert_eq!(
        otxn_field(&mut buf, hooks_lib::raw::sfcodes::sfAccount).err(),
        Some(STUB)
    );
    let stored: u32 = sfAccount.code();
    assert_eq!(otxn_field(&mut buf, stored).err(), Some(STUB));
}

#[test]
fn navigation_types() {
    fn _typed(root: SlotObject<STObject>) -> Result<()> {
        let _: SlotObject<u32> = root.get(sfSequence)?;
        let _: SlotObject<Amount> = root.get(sfBalance)?;
        let arr: SlotObject<STArray> = root.get(sfSignerEntries)?;
        let _n = arr.count()?;
        let _: SlotObject<STObject> = arr.get(0u32)?;
        // borrowing pre-checks compose with a consuming read
        let amt = root.get(sfBalance)?;
        let _is = amt.is_native()?;
        let _x: XFL = amt.as_xfl()?;
        // opaque both ways
        let op: SlotObject<Opaque> = root.get(sfSigningPubKey)?;
        let _: SlotObject<AccountId> = op.get(sfAccount)?;
        let _: SlotObject<STObject> = op.get(0u32)?;
        Ok(())
    }
    fn _reads(root: SlotObject<STObject>) -> Result<()> {
        let _: u32 = root.get(sfSequence)?.value()?;
        let _: u64 = root.get(sfExchangeRate)?.value()?;
        let _: AccountId = root.get(sfAccount)?.value()?;
        let _: Hash = root.get(sfLedgerHash)?.value()?;
        let _: CurrencyCode = root
            .get(sfTakerPaysCurrency)?
            .assume_type::<CurrencyCode>()
            .value()?;
        let _: AmountBytes = root.get(sfBalance)?.value()?;
        let _: IssueData = root.get(sfAsset)?.value()?;
        // take_* recycling
        let _: u32 = root.get(sfSequence)?.take_value()?;
        let _: XFL = root.get(sfBalance)?.take_xfl()?;
        let _: [u8; 4] = root.get(sfSequence)?.take_raw_exact::<4>()?;
        // raw escapes
        let mut b = [0u8; 8];
        let _n = root.get(sfSequence)?.raw(&mut b)?;
        let _: [u8; 32] = root.get(sfLedgerHash)?.raw_exact::<32>()?;
        // casts
        let _: SlotObject<STObject> = root.get(sfMemo)?.try_cast::<STObject>()?;
        let _: SlotObject<u32> = root.get(sfSigningPubKey)?.assume_type::<u32>();
        Ok(())
    }
    let _ = (_typed, _reads);
}

#[test]
fn slot_path_shapes() {
    fn _p(signers: SlotObject<STObject>) -> Result<()> {
        let one = slot_path!(signers[sfSignerEntries])?;
        let _ = one.clear();
        let three: SlotObject<AccountId> = slot_path!(signers[sfSignerEntries][0u32][sfAccount])?;
        let _: AccountId = three.value()?;
        Ok(())
    }
    let _ = _p;
    // runs on stubs: first hop fails, nothing leaks
    let r = SlotObject::from_otxn();
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// Field-table parity
// ---------------------------------------------------------------------------
//
// The full 325-name `typed.code() == raw` comparison is *generated* into
// `sfield.rs` alongside the table it checks (`cargo xtask gen-core`), so it
// cannot drift when upstream adds a field — run it with
// `cargo test -p hooks-lib --lib parity`. What is left here is the shape
// check the generated test cannot make: that both files declare the same
// set of names in the first place.

#[test]
fn both_tables_declare_the_same_names() {
    let typed = include_str!("../src/sfield.rs");
    let raw = include_str!("../../hooks-core/src/sfcodes.rs");

    let names = |src: &str| -> std::collections::BTreeSet<String> {
        src.lines()
            .filter_map(|l| l.trim().strip_prefix("pub const "))
            .filter_map(|l| l.split(':').next())
            .map(str::to_string)
            .collect()
    };
    let typed_names = names(typed);
    let raw_names = names(raw);

    assert_eq!(typed_names.len(), 325, "expected 325 typed constants");
    assert_eq!(
        typed_names, raw_names,
        "the typed and raw field tables declare different names",
    );
}

// ---------------------------------------------------------------------------
// slot_path!: root evaluated once, errors propagate per hop
// ---------------------------------------------------------------------------

#[test]
fn slot_path_evaluates_its_root_once() {
    use core::cell::Cell;

    // The counter has to be ticked by an expression the macro *itself*
    // evaluates — the parenthesized-root form — or this proves nothing about
    // the macro at all. `slot_path!((expr)[..])` binds that expression once;
    // a naive expansion that re-emitted `$root` per hop would tick twice.
    let calls = Cell::new(0u32);
    let make = || {
        calls.set(calls.get().wrapping_add(1));
        SlotObject::from_otxn()
    };

    // Wrapped so `?` inside the root expression has somewhere to go.
    let walk = || -> Result<SlotObject<STObject>> { slot_path!((make()?)[sfSignerEntries][0u32]) };
    assert_eq!(walk().err(), Some(STUB));
    assert_eq!(calls.get(), 1, "the root expression must be evaluated once");
}

#[test]
fn slot_path_propagates_the_failing_hop() {
    // On host stubs the very first hop fails, and the error surfaces
    // unchanged rather than being masked by an intermediate clear.
    fn walk(root: &SlotObject<STObject>) -> Result<SlotObject<AccountId>> {
        slot_path!(root[sfSignerEntries][0u32][sfAccount])
    }
    let _ = walk;
}
