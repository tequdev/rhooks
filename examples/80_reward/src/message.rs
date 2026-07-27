//! The `ClaimReward`-too-early rollback message: `"You must wait NNNNNNN
//! seconds"`, with the seven digits patched in place from the still-
//! required delay — matches reward.c's `msg_buf` byte-patching exactly
//! (digits at offsets 14..=20 of the template below), just expressed
//! without C's raw pointer arithmetic.

/// `"You must wait 0000000 seconds"` — digits at indices 14..=20.
const TEMPLATE: [u8; 29] = *b"You must wait 0000000 seconds";

/// Adds `digit` (0..=9) to the ASCII `'0'` already at `msg[index]`,
/// producing the ASCII digit character — a direct translation of
/// reward.c's `msg_buf[N] += digit`.
fn bump_digit(msg: &mut [u8; 29], index: usize, digit: u8) {
    if let Some(byte) = msg.get_mut(index) {
        *byte = byte.wrapping_add(digit);
    }
}

/// Builds the "still waiting" rollback message for `remaining_seconds`
/// (`required_delay - time_elapsed` in reward.c; always < 10_000_000 in
/// practice, since `required_delay` itself is a `ClaimReward` delay in
/// seconds, but each digit position is computed independently exactly as
/// reward.c does, so a larger value degrades the same way reward.c's does
/// — only the ones-through-millions digits are shown).
///
/// Deliberately seven explicit statements rather than a loop over
/// `[1_000_000, 100_000, ..., 1]`: the digit count is fixed by the
/// template's shape, not data-dependent, so writing it out hand-unrolled
/// (matching reward.c's own seven `msg_buf[N] +=` statements) needs no
/// `guard!` at all instead of a guarded loop.
#[must_use]
pub fn wait_message(remaining_seconds: u64) -> [u8; 29] {
    let mut msg = TEMPLATE;
    let r = remaining_seconds;
    bump_digit(
        &mut msg,
        14,
        r.wrapping_div(1_000_000).wrapping_rem(10) as u8,
    );
    bump_digit(&mut msg, 15, r.wrapping_div(100_000).wrapping_rem(10) as u8);
    bump_digit(&mut msg, 16, r.wrapping_div(10_000).wrapping_rem(10) as u8);
    bump_digit(&mut msg, 17, r.wrapping_div(1_000).wrapping_rem(10) as u8);
    bump_digit(&mut msg, 18, r.wrapping_div(100).wrapping_rem(10) as u8);
    bump_digit(&mut msg, 19, r.wrapping_div(10).wrapping_rem(10) as u8);
    bump_digit(&mut msg, 20, r.wrapping_rem(10) as u8);
    msg
}
