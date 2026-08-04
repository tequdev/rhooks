//! `XFL!` never silently rounds: more than 16 significant decimal digits
//! (after trailing-zero normalization) is a compile error, in both integer
//! and decimal form.

use hooks_lib::XFL;

fn main() {
    XFL!(12345678901234567);
    XFL!(1.2345678901234567);
}
