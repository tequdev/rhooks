//! Hook-state key layouts for governance state.

use rshooks::guard;
use rshooks::types::AccountId;

/// Current member count.
pub const MEMBER_COUNT: [u8; 2] = *b"MC";
/// L1 reward rate.
pub const REWARD_RATE: [u8; 2] = *b"RR";
/// L1 reward delay.
pub const REWARD_DELAY: [u8; 2] = *b"RD";

/// Maps a seat to a member.
pub fn seat_forward_key(seat: u8) -> [u8; 1] {
    [seat]
}

/// Maps a member to a seat.
pub fn member_reverse_key(account: &AccountId) -> [u8; 20] {
    account.0
}

/// Builds a member vote key.
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

/// Builds a vote-count key.
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
