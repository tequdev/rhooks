//! `XFL!` expands to `XFL::from_raw_bits`, a `const fn`, so it can populate
//! a `const`/`static` item directly -- no `from_raw_bits(<hand-computed
//! literal>)` detour needed.

use hooks_lib::XFL;
use hooks_lib::xfl::XFL as XflType;

const ONE: XflType = XFL!(1);
const NEGATIVE_ONE: XflType = XFL!(-1);
const TENTH: XflType = XFL!(0.1);
const REWARD_RATE: XflType = XFL!(0.003333333333333333);
const REWARD_DELAY: XflType = XFL!(2_600_000);
static VIA_EXPONENT: XflType = XFL!(2.6e6);

fn main() {
    assert_eq!(ONE.raw_bits(), 6_089_866_696_204_910_592);
    assert_eq!(NEGATIVE_ONE.raw_bits(), 1_478_180_677_777_522_688);
    assert_eq!(TENTH.raw_bits(), 6_071_852_297_695_428_608);
    assert_eq!(REWARD_RATE.raw_bits(), 6_038_156_834_009_797_973);
    assert_eq!(REWARD_DELAY.raw_bits(), 6_199_553_087_261_802_496);
    assert_eq!(VIA_EXPONENT.raw_bits(), REWARD_DELAY.raw_bits());

    // Zero forms all collapse to canonical zero.
    assert_eq!(XFL!(0).raw_bits(), 0);
    assert_eq!(XFL!(-0.0).raw_bits(), 0);
}
