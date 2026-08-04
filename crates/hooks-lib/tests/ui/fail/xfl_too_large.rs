//! A magnitude beyond XFL's representable range (unbiased exponent above
//! 80, roughly `1e96`) is a distinct "too large" compile error, in integer
//! and exponent form, positive and negative.

use hooks_lib::XFL;

fn main() {
    XFL!(1e96);
    XFL!(-1e96);
    XFL!(1_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000_000);
}
