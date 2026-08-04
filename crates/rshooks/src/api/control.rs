//! Hook execution flow control: `accept`, `rollback`, `hook_again`,
//! `hook_skip`, `hook_pos`.

use crate::error::{Result, res};

/// Terminate hook execution successfully, optionally carrying a UTF-8-ish
/// message and an application-defined return code.
///
/// On the real wasm host this call never returns (`accept` unwinds hook
/// execution). On host builds the underlying stub returns normally, so this
/// falls back to an explicit infinite loop purely to honor the `-> !`
/// signature without invoking real undefined behavior; that branch is
/// reachable only in host tests/doctests, never in a real wasm hook.
///
/// # Examples
///
/// ```no_run
/// use rshooks::api::control::accept;
///
/// accept(b"done", 0);
/// ```
#[inline(always)]
pub fn accept(msg: &[u8], code: i64) -> ! {
    unsafe {
        let _ = rshooks_core::accept(msg.as_ptr() as u32, msg.len() as u32, code);
    }
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::unreachable();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Host-only: the stub `accept` returns normally, so an explicit
        // infinite loop is the only panic-free way to honor `-> !` here.
        // Never reached in a real wasm hook (the arm above is used there).
        #[allow(clippy::empty_loop)]
        loop {}
    }
}

/// Terminate hook execution with a failure, rolling back all state changes
/// made by this hook invocation. See [`accept`] for the `-> !` rationale.
#[inline(always)]
pub fn rollback(msg: &[u8], code: i64) -> ! {
    unsafe {
        let _ = rshooks_core::rollback(msg.as_ptr() as u32, msg.len() as u32, code);
    }
    #[cfg(target_arch = "wasm32")]
    {
        core::arch::wasm32::unreachable();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // See the matching arm in `accept` above for why this is here.
        #[allow(clippy::empty_loop)]
        loop {}
    }
}

/// Request that the hook be called again after the originating transaction
/// completes (weak execution). Returns the raw success payload.
///
/// # Examples
///
/// ```
/// use rshooks::api::control::hook_again;
/// use rshooks::error::HookError;
///
/// assert_eq!(hook_again(), Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn hook_again() -> Result<i64> {
    res(unsafe { rshooks_core::hook_again() })
}

/// Instruct the enclosing hook chain to skip a specific hook (by hash) on
/// subsequent invocations, according to `flags`.
#[inline(always)]
pub fn hook_skip(hash: &[u8], flags: u32) -> Result<i64> {
    res(unsafe { rshooks_core::hook_skip(hash.as_ptr() as u32, hash.len() as u32, flags) })
}

/// Get the hook's position (index) in the hook chain of the current account.
///
/// Never returns a Hook API error code, so it is exposed as a plain `u8`
/// (a hook chain holds at most 10 hooks) rather than a `Result`.
#[inline(always)]
pub fn hook_pos() -> u8 {
    unsafe { rshooks_core::hook_pos() as u8 }
}
