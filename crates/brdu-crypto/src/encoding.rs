//! Deterministic encoding (ADR-0011): fields in fixed order, big-endian integers, a 4-byte length before
//! each variable field. There are never two ways to write the same value, so the hash is stable across machines.

use alloc::vec::Vec;

use crate::error::DecodeError;

/// Builds bytes field by field.
#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
    overflow: bool,
}

impl Encoder {
    /// An empty encoder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A single byte.
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    /// A 32-bit integer, big-endian.
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// A 64-bit integer, big-endian.
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    /// Bytes with the length (4 bytes) in front.
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        match u32::try_from(v.len()) {
            Ok(n) => {
                self.u32(n);
                self.buf.extend_from_slice(v);
            }
            Err(_) => self.overflow = true,
        }
        self
    }

    /// UTF-8 text with the length in front.
    pub fn string(&mut self, v: &str) -> &mut Self {
        self.bytes(v.as_bytes())
    }

    /// Fixed-size bytes, with the size known to the reader, without a prefix (keys, hashes, signatures).
    pub fn fixed(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }

    /// Number of items in a list (4 bytes); the items follow, one by one.
    pub fn count(&mut self, n: usize) -> &mut Self {
        match u32::try_from(n) {
            Ok(n) => {
                self.u32(n);
            }
            Err(_) => self.overflow = true,
        }
        self
    }

    /// Finishes. Fails only if some field exceeded 4 GiB (never happens with real messages).
    pub fn finish(self) -> Result<Vec<u8>, DecodeError> {
        if self.overflow {
            Err(DecodeError::TooLong)
        } else {
            Ok(self.buf)
        }
    }
}

/// Reads bytes field by field, in the same order the [`Encoder`] wrote them.
#[derive(Debug, Clone, Copy)]
pub struct Decoder<'a> {
    data: &'a [u8],
}

impl<'a> Decoder<'a> {
    /// Starts reading `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let (head, rest) = self
            .data
            .split_at_checked(n)
            .ok_or(DecodeError::Truncated)?;
        self.data = rest;
        Ok(head)
    }

    /// A single byte.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        self.take(1)?.first().copied().ok_or(DecodeError::Truncated)
    }

    /// A 32-bit integer.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(self.fixed::<4>()?))
    }

    /// A 64-bit integer.
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_be_bytes(self.fixed::<8>()?))
    }

    /// Exactly `N` bytes.
    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.take(N)?.try_into().map_err(|_| DecodeError::Truncated)
    }

    /// Bytes with a length in front; rejects more than `max` (allocates nothing before checking).
    pub fn bytes(&mut self, max: usize) -> Result<&'a [u8], DecodeError> {
        let n = usize::try_from(self.u32()?).map_err(|_| DecodeError::TooLong)?;
        if n > max {
            return Err(DecodeError::TooLong);
        }
        self.take(n)
    }

    /// UTF-8 text with a length in front; rejects more than `max` bytes.
    pub fn string(&mut self, max: usize) -> Result<&'a str, DecodeError> {
        core::str::from_utf8(self.bytes(max)?).map_err(|_| DecodeError::Utf8)
    }

    /// Number of items in a list; rejects more than `max` (each item takes at least 1 byte, so a
    /// count greater than the remaining bytes is truncated).
    pub fn count(&mut self, max: usize) -> Result<usize, DecodeError> {
        let n = usize::try_from(self.u32()?).map_err(|_| DecodeError::TooLong)?;
        if n > max {
            return Err(DecodeError::TooLong);
        }
        if n > self.data.len() {
            return Err(DecodeError::Truncated);
        }
        Ok(n)
    }

    /// How many bytes have not been read yet.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len()
    }

    /// Checks that nothing is left over.
    pub fn finish(self) -> Result<(), DecodeError> {
        if self.data.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::Trailing)
        }
    }
}
