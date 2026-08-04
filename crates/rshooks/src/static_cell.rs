//! [`HookStatic`]: safe, take-once access to `static` hook buffers.
//!
//! Constant templates and large zero-initialized buffers should live in
//! `static`s rather than stack locals (data segment / BSS instead of
//! runtime store chains or a compiler-generated memset — see
//! `docs/DESIGN.md` §6.3, "static-buffer idiom"). A bare `static mut`
//! makes that possible but forces every hook to repeat an unsafe,
//! clippy-fighting access incantation and offers no protection against
//! creating two aliasing `&mut` to the same buffer.
//!
//! `HookStatic<T>` wraps the buffer with a take-once flag: [`take`] hands
//! out the one-and-only `&'static mut T` and every later call returns
//! `None`. The single `unsafe` lives here, justified once; call sites are
//! plain safe Rust.
//!
//! [`take`]: HookStatic::take

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// A `static`-friendly cell handing out exclusive access to its contents
/// exactly once per instance lifetime.
///
/// Hooks run single-threaded and every hook invocation executes in a
/// freshly instantiated wasm instance, so "once per instance lifetime"
/// means "once per hook execution".
///
/// # Examples
///
/// ```
/// use rshooks::static_cell::HookStatic;
///
/// static BUF: HookStatic<[u8; 4]> = HookStatic::new([1, 2, 3, 4]);
///
/// let buf = BUF.take().expect("first take");
/// buf[0] = 9;
/// assert!(BUF.take().is_none()); // exclusive: handed out only once
/// ```
pub struct HookStatic<T> {
    // Atomic (not Cell) so `take` is race-free even on multi-threaded hosts
    // (tests, rust-analyzer); on single-threaded wasm it lowers to the same
    // plain load/store.
    taken: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: the only mutation reachable through `&self` is the atomic flag;
// `take` hands out the interior `&mut` exactly once (the swap has exactly
// one winner, on any number of threads), so shared references never expose
// aliased mutation.
unsafe impl<T: Send> Sync for HookStatic<T> {}

impl<T> HookStatic<T> {
    /// Creates a cell. `const`, so it can initialize a `static`: the value
    /// bytes land in a wasm data segment (or BSS when all-zero).
    pub const fn new(value: T) -> Self {
        Self {
            taken: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Returns the exclusive `&'static mut` to the contents on the first
    /// call, and `None` on every call after that.
    ///
    /// The take-once flag is what makes this safe: two aliasing `&mut` to
    /// the same static can never be produced. There is deliberately no
    /// "give back" operation — a hook runs once and exits.
    #[allow(clippy::mut_from_ref)] // uniqueness enforced by the take-once flag, not by the type system
    #[inline(always)]
    pub fn take(&'static self) -> Option<&'static mut T> {
        if self.taken.swap(true, Ordering::AcqRel) {
            None
        } else {
            // SAFETY: exactly one caller ever wins the swap above, so the
            // returned reference is unique.
            Some(unsafe { &mut *self.value.get() })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)] // tests are exempt from panic-freedom lints, docs/DESIGN.md §8

    use super::*;

    static CELL: HookStatic<[u8; 3]> = HookStatic::new([7, 8, 9]);

    #[test]
    fn take_yields_value_once_then_none() {
        let first = CELL.take();
        let second = CELL.take();
        // Exactly one of the two calls got the buffer (test order within
        // this fn is deterministic: the first).
        let buf = first.expect("first take yields the buffer");
        assert_eq!(buf, &mut [7, 8, 9]);
        buf[0] = 42;
        assert!(second.is_none());
        assert!(CELL.take().is_none());
    }

    #[test]
    fn take_is_exclusive_across_threads() {
        extern crate std;
        use std::{thread, vec::Vec};

        static RACE: HookStatic<u32> = HookStatic::new(0);

        let handles: Vec<_> = (0..8)
            .map(|_| thread::spawn(|| RACE.take().is_some()))
            .collect();
        let winners = handles
            .into_iter()
            .map(|h| h.join())
            .filter(|r| matches!(r, Ok(true)))
            .count();
        assert_eq!(winners, 1, "exactly one thread may win the take");
    }
}
