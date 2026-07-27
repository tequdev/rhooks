//! Hook-state key layouts, transcribed exactly from govern.c's own key
//! construction (see the module-level state-layout table in the crate
//! README). Every function here returns a plain byte buffer — no
//! `hooks_lib::types::StateKey` newtype, because govern.c's keys are not
//! uniformly 32 bytes (`"MC"` is 2 bytes, a seat-forward key is 1 byte,
//! a member-reverse key is 20 bytes; only the vote/vote-count keys are a
//! fixed 32 bytes) and `hooks_lib::api::state::state`'s key parameter
//! (`AsRef<[u8]>`) already accepts any length directly.

use hooks_lib::guard;
use hooks_lib::types::AccountId;

/// `{'M', 'C'}` — current member count (1 byte value).
pub const MEMBER_COUNT: [u8; 2] = *b"MC";
/// `{'R', 'R'}` — current reward rate (8-byte LE XFL value, L1 table only).
pub const REWARD_RATE: [u8; 2] = *b"RR";
/// `{'R', 'D'}` — current reward delay (8-byte LE XFL value, L1 table
/// only).
pub const REWARD_DELAY: [u8; 2] = *b"RD";

/// `{seat}` (1 byte) -> 20-byte account ID: forward seat -> member lookup.
pub fn seat_forward_key(seat: u8) -> [u8; 1] {
    [seat]
}

/// `{20-byte account ID}` -> 1-byte seat: reverse member -> seat lookup.
pub fn member_reverse_key(account: &AccountId) -> [u8; 20] {
    account.0
}

/// `{'V', topic_type, topic_id, layer, 0*8, 20-byte voter account}` (32
/// bytes) — a member's vote for a topic. Always this exact fixed shape
/// (fixed 20-byte account payload, independent of the *topic's own* data
/// size) — govern.c's `account_field[0..4]` header overwrite on top of
/// the already-`account_field[12..32)`-populated voter account.
pub fn vote_key(topic_type: u8, topic_id: u8, layer: u8, voter: &AccountId) -> [u8; 32] {
    let mut k = [0u8; 32];
    k[0] = b'V';
    k[1] = topic_type;
    k[2] = topic_id;
    k[3] = layer;
    let mut i = 0usize;
    while i < 20 {
        guard!(20);
        if let (Some(slot), Some(&b)) = (k.get_mut(12usize.wrapping_add(i)), voter.0.get(i)) {
            *slot = b;
        }
        i = i.wrapping_add(1);
    }
    k
}

/// `{'C', topic_type, topic_id, layer, <front-truncated topic data>}` (32
/// bytes) — the vote count for a topic+data combination. `value` (the
/// topic's own data, 8/20/32 bytes) is written **first**, right-aligned
/// (front-padded with zeros for a padding topic size), and the 4-byte
/// header is written **second**, directly on top — reproducing govern.c's
/// own in-place clobber of `topic_data`'s first 4 bytes (`topic_data[0] =
/// 'C'; ...`), which for the 32-byte (`'H'`) topic size genuinely
/// overwrites the first 4 bytes of the hook-hash value itself. See the
/// README's differences table: this collision-prone quirk is preserved
/// exactly for state-key parity with govern.c, not accidentally.
pub fn vote_count_key(topic_type: u8, topic_id: u8, layer: u8, value: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 32];
    let start = 32usize.saturating_sub(value.len());
    let mut i = 0usize;
    while i < value.len() {
        guard!(32);
        if let (Some(slot), Some(&b)) = (k.get_mut(start.wrapping_add(i)), value.get(i)) {
            *slot = b;
        }
        i = i.wrapping_add(1);
    }
    k[0] = b'C';
    k[1] = topic_type;
    k[2] = topic_id;
    k[3] = layer;
    k
}
