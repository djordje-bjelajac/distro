//! Lowercase hex for the two 32-byte values these files hold: a `PeerId`'s
//! public key and the keystore's secret seed.
//!
//! Hex rather than base64 or raw bytes because every one of these files is
//! line-based text, and a fixed-width, case-insensitive, delimiter-free
//! encoding is the one that survives being looked at, grepped, and pasted into
//! a bug report without an escaping rule. 64 characters per key is not a size
//! that matters for files holding tens of entries.

/// The 32-byte values these files hold: an Ed25519 public key or secret seed.
pub(crate) const KEY_BYTES: usize = 32;

/// Renders `bytes` as 64 lowercase hex characters.
pub(crate) fn encode(bytes: &[u8; KEY_BYTES]) -> String {
    let mut text = String::with_capacity(KEY_BYTES * 2);

    for byte in bytes {
        text.push(digit(byte >> 4));
        text.push(digit(byte & 0x0f));
    }

    text
}

/// Parses 64 hex characters back into 32 bytes.
///
/// Accepts either case on the way in while only ever emitting lowercase: a
/// reader that refused an uppercase file would turn a cosmetic difference into
/// a lost identity, and there is no ambiguity to resolve.
pub(crate) fn decode(text: &str) -> Option<[u8; KEY_BYTES]> {
    if text.len() != KEY_BYTES * 2 {
        return None;
    }

    let mut bytes = [0u8; KEY_BYTES];
    let mut characters = text.chars();

    for byte in &mut bytes {
        let high = value(characters.next()?)?;
        let low = value(characters.next()?)?;
        *byte = (high << 4) | low;
    }

    Some(bytes)
}

const fn digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'a' + nibble - 10) as char,
    }
}

const fn value(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(character as u8 - b'0'),
        'a'..='f' => Some(character as u8 - b'a' + 10),
        'A'..='F' => Some(character as u8 - b'A' + 10),
        _ => None,
    }
}
