//! A magnitude below XFL's representable range (unbiased exponent below
//! -96, roughly `1e-81`) is a distinct "too small" compile error -- not
//! silently flushed to zero.

use rshooks::XFL;

fn main() {
    XFL!(9.99e-82);
    XFL!(-9.99e-82);
    XFL!(1e-200);
}
