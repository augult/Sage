THOUGHT = "The iterator's closure captures `&mut self`, so the opaque type also captures the key lifetime `'a`. Name it explicitly with precise capture (`use<'a, 'b, Data>`), keeping the `IntoIter: 'b` bound, then re-run the full io-feature check."
CONTENT = "Fixed `munge`'s return type to explicitly capture the key lifetime along with the borrow and iterator type."
CALLS = [
    {
        "name": "edit",
        "arguments": {
            "path": "src/lib.rs",
            "old": r'''    pub fn munge<'b, Data>(&'b mut self, data: Data) -> impl Iterator<Item = u8> + 'b
    where
        Data: IntoIterator,
        Data::Item: Borrow<u8>,
        Data::IntoIter: 'b,
    {''',
            "new": r'''    pub fn munge<'b, Data>(&'b mut self, data: Data) -> impl Iterator<Item = u8> + 'b + use<'a, 'b, Data>
    where
        Data: IntoIterator,
        Data::Item: Borrow<u8>,
        Data::IntoIter: 'b,
    {''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''mkdir -p tests && cat > tests/check_tmp.rs <<'EOF'
use std::io::{Read, Write};
use xorcism::Xorcism;

#[test]
fn munge_in_place_cyclically() {
    let mut munger = Xorcism::new(b"abc");
    let mut data = *b"1234567";
    munger.munge_in_place(&mut data);
    assert_eq!(&data, b"PPPUWUV");
}

#[test]
fn munge_cyclically() {
    let mut munger = Xorcism::new(b"abc");
    assert_eq!(munger.munge(b"1234567").collect::<Vec<_>>(), b"PPPUWUV");
}

#[test]
fn empty_inputs() {
    let mut munger = Xorcism::new(b"abc");
    let mut data: [u8; 0] = [];
    munger.munge_in_place(&mut data);
    assert_eq!(munger.munge(Vec::<u8>::new()).count(), 0);
}

#[test]
fn stateful_across_calls() {
    let mut munger = Xorcism::new(b"abc");
    assert_eq!(munger.munge(b"1234").collect::<Vec<_>>(), b"PPPU");
    assert_eq!(munger.munge(b"567").collect::<Vec<_>>(), b"WUV");
}

#[test]
fn round_trip_and_key_types() {
    let key = b"secret".to_vec();
    let mut enc = Xorcism::new(&key);
    let cipher: Vec<u8> = enc.munge(b"hello world").collect();
    let mut dec = Xorcism::new(&key);
    assert_eq!(dec.munge(cipher).collect::<Vec<_>>(), b"hello world");
    let mut s = Xorcism::new("abc");
    assert_eq!(s.munge(b"1").collect::<Vec<_>>(), b"P");
}

#[test]
fn reader_adaptor() {
    let mut reader = Xorcism::new(b"abc").reader(&b"1234567"[..]);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out, b"PPPUWUV");
}

#[test]
fn writer_adaptor_stateful_chunks() {
    let mut out = Vec::new();
    {
        let mut writer = Xorcism::new(b"abc").writer(&mut out);
        writer.write_all(b"1234").unwrap();
        writer.write_all(b"567").unwrap();
        writer.flush().unwrap();
    }
    assert_eq!(out, b"PPPUWUV");
}
EOF
cargo test --features io --test check_tmp 2>&1 | tail -20; status=${PIPESTATUS[0]}; rm -f tests/check_tmp.rs; rmdir tests 2>/dev/null; exit $status''',
            "expect_exit": "0",
            "expect_has": "test result: ok. 7 passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `Xorcism` in `src/lib.rs`: it borrows the key via `AsRef<[u8]>` (accepting arrays, slices, `Vec<u8>`, and `&str`) and tracks a cycling key position so `munge_in_place` and `munge` are stateful across calls. `munge_in_place` XORs in place, `munge` is a lazy iterator accepting owned or borrowed byte items, and both work with empty inputs. The `io` bonus adds `reader`/`writer` adaptors over `Read`/`Write`; writes XOR into a stack scratch buffer so multi-call and chunked writes keep continuous key state with no heap allocation. Verified with a temporary integration test running `cargo test --features io` covering key cycling, statefulness across calls, empty input, owned/borrowed items and key types, round-trip, and reader/writer chunked state; the temporary test was removed.",
            "unverified": "",
        },
    },
]
