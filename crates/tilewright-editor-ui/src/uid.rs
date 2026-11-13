// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Uids for newly placed objects. World never assigns one, so the editor
//! writes `o.` and 128 random bits in lowercase base32, as the World format
//! recommends: independent of the object's place and of each other, so uids
//! written on different branches do not collide.
use std::fmt;

/// Prefix of every generated uid.
pub const UID_PREFIX: &str = "o.";
/// Base32 characters after the prefix: 128 bits in 5-bit groups.
pub const UID_CHARACTERS: usize = 26;
/// Generated uids already present in the World are drawn again at most
/// this many times in all before placement is refused.
pub const MAXIMUM_UID_ATTEMPTS: usize = 8;

/// RFC 4648 base32 in lowercase: every character is valid in a World
/// identifier (`^[a-z0-9][a-z0-9_.-]{0,127}$`).
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A source of 128-bit uid material. The Session uses the operating
/// system's random source; tests and the scripted demonstration inject a
/// deterministic one.
pub trait UidSource {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), UidError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UidError(pub String);
impl fmt::Display for UidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for UidError {}

/// The operating system's random source.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsUidSource;
impl UidSource for OsUidSource {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), UidError> {
        getrandom::fill(bytes)
            .map_err(|error| UidError(format!("operating system random source failed: {error}")))
    }
}

/// A deterministic sequence (`SplitMix64`) for tests and the scripted
/// demonstration. Never use it for authored worlds: its uids are
/// predictable and repeat between runs with the same seed.
#[derive(Clone, Copy, Debug)]
pub struct SeededUidSource {
    state: u64,
}
impl SeededUidSource {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}
impl UidSource for SeededUidSource {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), UidError> {
        bytes[..8].copy_from_slice(&self.next().to_be_bytes());
        bytes[8..].copy_from_slice(&self.next().to_be_bytes());
        Ok(())
    }
}

/// `o.` and the 128 bits in lowercase base32, most significant bits first;
/// the last character carries the final 3 bits and two zero bits.
pub fn format_uid(bytes: [u8; 16]) -> String {
    let value = u128::from_be_bytes(bytes);
    let mut uid = String::with_capacity(UID_PREFIX.len() + UID_CHARACTERS);
    uid.push_str(UID_PREFIX);
    for index in 0..UID_CHARACTERS {
        // Character i holds bits 123-5i..=127-5i of the value (bit 0 the
        // lowest); the last one is the low 3 bits followed by two zeros.
        let shift = 123i32 - 5 * i32::try_from(index).unwrap_or(0);
        let group = if shift >= 0 {
            value >> shift
        } else {
            value << -shift
        };
        uid.push(char::from(
            ALPHABET[usize::try_from(group & 31).unwrap_or(0)],
        ));
    }
    uid
}

/// Draw uids from `source` until one is not `taken`, at most
/// [`MAXIMUM_UID_ATTEMPTS`] times.
pub fn unused_uid(
    source: &mut dyn UidSource,
    taken: impl Fn(&str) -> bool,
) -> Result<String, UidError> {
    for _ in 0..MAXIMUM_UID_ATTEMPTS {
        let mut bytes = [0; 16];
        source.fill(&mut bytes)?;
        let uid = format_uid(bytes);
        if !taken(&uid) {
            return Ok(uid);
        }
    }
    Err(UidError(format!(
        "no unused uid after {MAXIMUM_UID_ATTEMPTS} attempts; nothing was prepared"
    )))
}

#[cfg(test)]
mod tests {
    use super::{UID_CHARACTERS, format_uid};

    #[test]
    fn base32_places_every_bit_most_significant_first() {
        assert_eq!(format_uid([0; 16]), format!("o.{}", "a".repeat(26)));
        assert_eq!(format_uid([0xff; 16]), format!("o.{}4", "7".repeat(25)));
        let mut top = [0; 16];
        top[0] = 0x80;
        assert_eq!(format_uid(top), format!("o.q{}", "a".repeat(25)));
        let mut low = [0; 16];
        low[15] = 1;
        assert_eq!(format_uid(low), format!("o.{}e", "a".repeat(25)));
        let uid = format_uid([0x5a; 16]);
        assert_eq!(uid.len(), 2 + UID_CHARACTERS);
        assert!(tilewright_source::is_identifier(&uid));
    }
}
