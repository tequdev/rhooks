//! Everything that is not "an optional leading `-` plus exactly one plain
//! decimal numeric literal": a string literal, extra trailing tokens, a
//! numeric type suffix, a hexadecimal literal, and no input at all.

use rshooks::XFL;

fn main() {
    XFL!("0.1");
    XFL!(1, 2);
    XFL!(1.0f64);
    XFL!(0x1A);
    XFL!();
    // `.5` tokenizes as a `.` `Punct` followed by a `5` `Literal` -- two
    // tokens, not one numeric literal -- so this hits the same "expects a
    // single numeric literal" path as the other non-literal shapes above.
    XFL!(.5);
}
