//! `guard-patterns` — a teaching example (not a realistic policy on its
//! own) for the Hook API's loop-guard system: correct `guard!`/`guard_m!`
//! usage, how to choose `maxiter`, and the `[u8; N] == [u8; N]`
//! compiler-generated-loop pitfall and how to avoid it. See the README for
//! the measured worst-case instruction count this produces.
//!
//! Build: `hooks-build build --manifest-path examples/06_guard-patterns/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, guard, guard_m, hook_errors, rollback};

/// Name of the Hook parameter carrying the 20-byte blocked `AccountId`
/// (same idiom as `firewall`'s `BL` parameter).
const BL_PARAM: &[u8] = b"BL";

hook_errors! {
    /// `guard-patterns` rollback codes.
    pub enum GuardPatternsError {
        /// `otxn_field(sfAccount)` did not return a 20-byte `AccountId`.
        CouldNotReadSender = 1,
        /// The originating transaction's sender matched the blacklisted
        /// account configured via the `BL` Hook parameter.
        BlockedAccount = 2,
    }
}

/// Compares two `AccountId`s byte-by-byte with an explicit, guarded loop —
/// **the correct way to compare fixed-size byte arrays in a Hook**, and the
/// only kind of comparison this crate ever performs on one.
///
/// # Why not `==`
///
/// It is tempting to just write `*a == *b`. Don't: on `wasm32v1-none`
/// (WASM MVP only, no bulk-memory instructions), LLVM at `opt-level = "z"`
/// lowers a `[u8; 20]` equality check to a call into a `compiler_builtins`
/// `bcmp`-style function containing a real, unguarded loop — one that never
/// appears as a `loop` keyword anywhere in this crate's source. `firewall`
/// (`examples/05_firewall`) does exactly this and, as a result, needs
/// `hooks-build build --auto-guard --default-maxiter 24` to pass at all:
/// the guard pass has to notice the compiler-generated loop after the
/// fact and insert a guard for it, with a `maxiter` the developer has to
/// separately verify against the loop's *actual* worst case (20 iterations,
/// one per byte) — `--auto-guard`'s own default of 16 would build
/// successfully yet risk a real on-ledger `GUARD_VIOLATION`. See
/// `examples/README.md`'s "On `--auto-guard`" section for the full story.
///
/// The loop below sidesteps all of that: it's written by hand, so its
/// `guard!` is present in the source (no `--auto-guard` needed for this
/// function at all), and its `maxiter` is provably exact — see the comment
/// on the `guard!` call itself.
fn accounts_equal(a: &AccountId, b: &AccountId) -> bool {
    let mut i: usize = 0;
    loop {
        // `maxiter = ACC_ID_LEN` (20): this loop's true worst case is
        // *exactly* one iteration per `AccountId` byte, no more and no
        // less — unlike a loop bound that depends on runtime data (where
        // some slack above the expected case is often unavoidable), a
        // fixed-size array's length is a compile-time fact, so the
        // tightest correct `maxiter` is also the exact one. Getting this
        // right (rather than guessing something "safely large") is what
        // keeps the reported worst-case instruction count meaningful: it
        // reflects what this loop can actually cost, not a pessimistic
        // guess layered on top of an already-pessimistic guess.
        guard!(ACC_ID_LEN as u32);
        if i >= ACC_ID_LEN {
            break true;
        }
        if a.get(i) != b.get(i) {
            break false;
        }
        i = i.wrapping_add(1);
    }
}

/// Hook entry point.
///
/// Beyond the `accounts_equal` guarded comparison above, this function also
/// contains a second, deliberately contrived demonstration: two small
/// byte-summing loops, written on the *same physical source line* purely so
/// their `guard_m!` calls collide on `line!()` and need the `$n`
/// disambiguator to stay distinct. In real (non-teaching) code this
/// situation arises from *generated* code — e.g. a macro like
/// `hooks_lib::txn_template!` that expands to more than one loop at a
/// single call site — not from manually cramming code onto one line; it's
/// done directly here only so the collision is visible without a second
/// macro layer standing between the reader and `line!()`'s actual behavior.
#[unsafe(no_mangle)]
#[rustfmt::skip] // see the doc comment above: staying on one physical line is the point
pub extern "C" fn hook(_reserved: u32) -> i64 {
    let sender: AccountId = match otxn_field_exact::<ACC_ID_LEN>(sfAccount) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"guard-patterns: could not read otxn sender",
            GuardPatternsError::CouldNotReadSender
        ),
    };

    // No (valid) blacklist parameter configured: nothing to block.
    let Ok(blocked) = hook_param_exact::<ACC_ID_LEN>(BL_PARAM) else {
        accept!();
    };

    if accounts_equal(&sender, &blocked) {
        rollback!(
            b"guard-patterns: blocked account",
            GuardPatternsError::BlockedAccount
        );
    }

    // The `guard_m!` demonstration: two independent loops, each summing up
    // to 8 bytes of one of the two accounts above, both `loop { ... }`
    // blocks starting on the very same source line. Without the `1`/`2`
    // disambiguator, `guard!`'s id formula (`(1 << 31) + line!()`) would
    // assign these two, textually-distinct loops the *same* guard id.
    //
    // Verified empirically (not just by reading the formula): changing
    // either disambiguator below so both loops share one id still passes
    // `hooks-build build` — the static checker only verifies loop *shape*
    // (a guard call at the top of every loop), never that ids are unique.
    // The hazard `guard_m!` actually guards against is a **runtime** one:
    // `_g` tracks each guard id's iteration count as the hook executes, so
    // two unrelated loops sharing an id would share one counter, and
    // whichever runs first could push it toward the *other* loop's
    // `maxiter`, risking a spurious on-ledger `GUARD_VIOLATION` that no
    // build-time tool here can catch. Keep loop ids unique — the `$n` in
    // `guard_m!` is how you do that when two loops land on one line.
    //
    // Neither sum feeds back into the accept/rollback decision above; they
    // exist only to be guarded correctly and to end up in this hook's
    // measured worst-case instruction count (see the README).
    let mut i: usize = 0; let mut sum_a: u32 = 0; loop { guard_m!(8, 1); if i >= 8 { break; } sum_a = sum_a.wrapping_add(u32::from(sender.get(i).copied().unwrap_or(0))); i = i.wrapping_add(1); }
    let mut j: usize = 0; let mut sum_b: u32 = 0; loop { guard_m!(8, 2); if j >= 8 { break; } sum_b = sum_b.wrapping_add(u32::from(blocked.get(j).copied().unwrap_or(0))); j = j.wrapping_add(1); }

    accept!(b"guard-patterns: accepted", i64::from(sum_a.wrapping_add(sum_b)))
}
