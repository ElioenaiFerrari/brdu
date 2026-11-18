//! Lowercase hexadecimal (no dependencies).

use alloc::string::String;
use alloc::vec::Vec;

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Bytes as lowercase hexadecimal.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parses hexadecimal (upper or lower case). `None` if the length is odd or a character is invalid.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    pairs
        .iter()
        .map(|[hi, lo]| Some((nibble(*hi)? << 4) | nibble(*lo)?))
        .collect()
}

/// Parses exactly `N` bytes of hexadecimal.
#[must_use]
pub fn decode_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    decode(text)?.try_into().ok()
}
