//! Ergonomic wrappers over every `hooks-core` Hook API function (except
//! `_g`, which is only exposed via the `guard!`/`guard_m!` macros in
//! `macros.rs`), organized into one module per Hook API category — mirrors
//! the grouping in `hook/extern.h` and DESIGN.md §5.
//!
//! 60 of the 74 non-`_g` functions get a public wrapper here; the remaining
//! 14 (`float_set`, `float_multiply`, `float_mulratio`, `float_negate`,
//! `float_compare`, `float_sum`, `float_invert`, `float_divide`,
//! `float_one`, `float_mantissa`, `float_sign`, `float_int`, `float_log`,
//! `float_root`) are wrapped privately as [`crate::xfl::XFL`] methods
//! instead — see `xfl.rs`.

pub mod control;
pub mod etxn;
pub mod float;
pub mod hook_ctx;
pub mod ledger;
pub mod otxn;
pub mod slot;
pub mod state;
pub mod sto;
pub mod trace;
pub mod util;

pub use control::*;
pub use etxn::*;
pub use float::*;
pub use hook_ctx::*;
pub use ledger::*;
pub use otxn::*;
pub use slot::*;
pub use state::*;
pub use sto::*;
pub use trace::*;
pub use util::*;
