#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::DecodeError;
use brdu_crypto::encoding::{Decoder, Encoder};
use proptest::prelude::*;

#[test]
fn fixed_big_endian_format_with_length_prefix() {
    let mut e = Encoder::new();
    e.u8(1)
        .u32(0x0102_0304)
        .u64(5)
        .string("hi")
        .fixed(&[9, 9])
        .count(2);
    assert_eq!(
        e.finish().unwrap(),
        [
            1, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 2, b'h', b'i', 9, 9, 0, 0, 0, 2
        ]
    );
}

#[test]
fn round_trip() {
    let mut e = Encoder::new();
    e.u8(7)
        .u64(u64::MAX)
        .bytes(&[1, 2, 3])
        .string("hello, 世界")
        .fixed(&[0xaa; 4]);
    let bytes = e.finish().unwrap();
    let mut d = Decoder::new(&bytes);
    assert_eq!(d.u8(), Ok(7));
    assert_eq!(d.u64(), Ok(u64::MAX));
    assert_eq!(d.bytes(10), Ok(&[1u8, 2, 3][..]));
    assert_eq!(d.string(100), Ok("hello, 世界"));
    assert_eq!(d.fixed::<4>(), Ok([0xaa; 4]));
    assert_eq!(d.finish(), Ok(()));
}

#[test]
fn truncated_trailing_utf8_and_limit() {
    assert_eq!(Decoder::new(&[0, 0, 0]).u32(), Err(DecodeError::Truncated));
    assert_eq!(
        Decoder::new(&[0, 0, 0, 9, 1]).bytes(100),
        Err(DecodeError::Truncated)
    );
    assert_eq!(Decoder::new(&[1, 2]).finish(), Err(DecodeError::Trailing));
    assert_eq!(
        Decoder::new(&[0, 0, 0, 2, 0xff, 0xfe]).string(10),
        Err(DecodeError::Utf8)
    );
    // huge declared size: rejects WITHOUT trying to allocate anything
    assert_eq!(
        Decoder::new(&[0xff, 0xff, 0xff, 0xff]).bytes(1000),
        Err(DecodeError::TooLong)
    );
    assert_eq!(
        Decoder::new(&[0, 0, 0, 5, 1, 2, 3, 4, 5]).bytes(4),
        Err(DecodeError::TooLong)
    );
}

#[test]
fn count_greater_than_remaining_bytes_is_rejected() {
    // 1 million items declared with 2 bytes left: no allocating a 1-million vector
    let data = [0, 0x0f, 0x42, 0x40, 1, 2];
    assert_eq!(
        Decoder::new(&data).count(u32::MAX as usize),
        Err(DecodeError::Truncated)
    );
    assert_eq!(Decoder::new(&data).count(10), Err(DecodeError::TooLong));
}

proptest! {
    #[test]
    fn any_sequence_of_fields_round_trips(
        a in any::<u8>(), b in any::<u32>(), c in any::<u64>(),
        d in proptest::collection::vec(any::<u8>(), 0..200), s in ".{0,40}"
    ) {
        let mut e = Encoder::new();
        e.u8(a).u32(b).u64(c).bytes(&d).string(&s);
        let bytes = e.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        prop_assert_eq!(dec.u8(), Ok(a));
        prop_assert_eq!(dec.u32(), Ok(b));
        prop_assert_eq!(dec.u64(), Ok(c));
        prop_assert_eq!(dec.bytes(1000), Ok(d.as_slice()));
        prop_assert_eq!(dec.string(1000), Ok(s.as_str()));
        prop_assert_eq!(dec.finish(), Ok(()));
    }

    // a decoder never panics, whatever the input
    #[test]
    fn garbage_never_panics(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        let mut d = Decoder::new(&data);
        let _ = d.u8();
        let _ = d.bytes(16);
        let _ = d.string(16);
        let _ = d.count(8);
        let _ = d.fixed::<8>();
        let _ = d.finish();
    }
}
