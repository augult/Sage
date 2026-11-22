THOUGHT = "The diff is unchanged since my passing check. Re-run a self-contained edge-case check written outside the repo's test tree, covering every README example, u32::MAX round-trip, multi-value streams, empty inputs, and incomplete numbers, then finish."
CONTENT = "Re-verified all README vectors, u32::MAX, batch streams, empty input, and incomplete-number handling; no further code changes were needed."
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''mkdir -p tests && cat > tests/check_tmp.rs <<'EOF'
use variable_length_quantity::{from_bytes, to_bytes, Error};

#[test]
fn readme_vectors() {
    let cases: &[(u32, &[u8])] = &[
        (0x00000000, &[0x00]),
        (0x00000040, &[0x40]),
        (0x0000007F, &[0x7F]),
        (0x00000080, &[0x81, 0x00]),
        (0x00002000, &[0xC0, 0x00]),
        (0x00003FFF, &[0xFF, 0x7F]),
        (0x00004000, &[0x81, 0x80, 0x00]),
        (0x00100000, &[0xC0, 0x80, 0x00]),
        (0x001FFFFF, &[0xFF, 0xFF, 0x7F]),
        (0x00200000, &[0x81, 0x80, 0x80, 0x00]),
        (0x08000000, &[0xC0, 0x80, 0x80, 0x00]),
        (0x0FFFFFFF, &[0xFF, 0xFF, 0xFF, 0x7F]),
        (0xFFFFFFFF, &[0x8F, 0xFF, 0xFF, 0xFF, 0x7F]),
    ];
    for &(value, encoded) in cases {
        assert_eq!(to_bytes(&[value]), encoded, "encode {value:#010X}");
        assert_eq!(from_bytes(encoded), Ok(vec![value]), "decode {value:#010X}");
    }
}

#[test]
fn streams_and_edges() {
    let values = [0, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1FFFFF, 0x200000, 0xFFFFFFFF];
    assert_eq!(from_bytes(&to_bytes(&values)), Ok(values.to_vec()));
    assert_eq!(to_bytes(&[]), Vec::<u8>::new());
    assert_eq!(from_bytes(&[]), Ok(vec![]));
    assert_eq!(from_bytes(&[0xFF, 0xFF]), Err(Error::IncompleteNumber));
}
EOF
cargo test --test check_tmp 2>&1 | tail -12; status=${PIPESTATUS[0]}; rm -f tests/check_tmp.rs; rmdir tests 2>/dev/null; exit $status''',
            "expect_exit": "0",
            "expect_has": "test result: ok. 2 passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented VLQ encoding in `to_bytes` (7-bit groups, high bit set on all but the final byte, up to 5 bytes for u32) and decoding in `from_bytes` (accumulate 7-bit groups; return `Error::IncompleteNumber` when the stream ends with a continuation byte). Verified every README vector, u32::MAX encode/decode, multi-value streams, empty input, and trailing-continuation errors with a temporary test that was removed afterward.",
            "unverified": "",
        },
    },
]
