//! SetHook fee estimation: `docs/DESIGN.md` §6.1 — `bytes * 5000` drops.

/// Drops of XAH per byte of hook binary, per SetHook's fee schedule.
pub const DROPS_PER_BYTE: u64 = 5000;

/// Drops per whole XAH (1 XAH = 1,000,000 drops).
pub const DROPS_PER_XAH: u64 = 1_000_000;

/// A fee estimate for a hook binary of a given size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeEstimate {
    /// The size of the binary, in bytes.
    pub bytes: u64,
    /// The estimated SetHook fee, in drops.
    pub drops: u64,
}

impl FeeEstimate {
    /// The estimated fee, in whole XAH plus remainder drops, as a decimal
    /// string (e.g. `"1.234500"`).
    #[must_use]
    pub fn xah_string(&self) -> String {
        let whole = self.drops / DROPS_PER_XAH;
        let frac = self.drops % DROPS_PER_XAH;
        format!("{whole}.{frac:06}")
    }
}

/// Estimates the SetHook fee for a binary of the given size.
#[must_use]
pub fn estimate_fee(size_bytes: usize) -> FeeEstimate {
    let bytes = size_bytes as u64;
    FeeEstimate {
        bytes,
        drops: bytes.saturating_mul(DROPS_PER_BYTE),
    }
}
