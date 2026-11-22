THOUGHT = "The iterator returned by `munge` borrows `self`, so the associated `IntoIter` type must also outlive that borrow. Add the explicit lifetime and `Data::IntoIter: 'b` bound, then re-run the full io-feature check."
CONTENT = "Fixed the `munge` lifetime bound so the returned iterator's source outlives the mutable borrow."
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "src/lib.rs",
            "overwrite": True,
            "content": r'''use std::borrow::Borrow;
use std::io::{self, Read, Write};

/// A munger which XORs a key with some data
#[derive(Clone)]
pub struct Xorcism<'a> {
    key: &'a [u8],
    pos: usize,
}

impl<'a> Xorcism<'a> {
    /// Create a new Xorcism munger from a key
    ///
    /// Should accept anything which has a cheap conversion to a byte slice.
    pub fn new<Key>(key: &'a Key) -> Xorcism<'a>
    where
        Key: AsRef<[u8]> + ?Sized,
    {
        Xorcism {
            key: key.as_ref(),
            pos: 0,
        }
    }

    fn next_key_byte(&mut self) -> u8 {
        let byte = self.key[self.pos];
        self.pos = (self.pos + 1) % self.key.len();
        byte
    }

    /// XOR each byte of the input buffer with a byte from the key.
    ///
    /// Note that this is stateful: repeated calls are likely to produce different results,
    /// even with identical inputs.
    pub fn munge_in_place(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            *byte ^= self.next_key_byte();
        }
    }

    /// XOR each byte of the data with a byte from the key.
    ///
    /// Note that this is stateful: repeated calls are likely to produce different results,
    /// even with identical inputs.
    ///
    /// Should accept anything which has a cheap conversion to a byte iterator.
    /// Shouldn't matter whether the byte iterator's values are owned or borrowed.
    pub fn munge<Data>(&mut self, data: Data) -> impl Iterator<Item = u8> + '_
    where
        Data: IntoIterator,
        Data::Item: Borrow<u8>,
    {
        data.into_iter()
            .map(move |byte| *byte.borrow() ^ self.next_key_byte())
    }

    /// Convert this munger into a reader adaptor.
    pub fn reader<R>(self, inner: R) -> XorcismReader<'a, R>
    where
        R: Read,
    {
        XorcismReader { inner, munger: self }
    }

    /// Convert this munger into a writer adaptor.
    pub fn writer<W>(self, inner: W) -> XorcismWriter<'a, W>
    where
        W: Write,
    {
        XorcismWriter { inner, munger: self }
    }
}

/// Reader adaptor produced by [`Xorcism::reader`].
pub struct XorcismReader<'a, R> {
    inner: R,
    munger: Xorcism<'a>,
}

impl<R: Read> Read for XorcismReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.munger.munge_in_place(&mut buf[..read]);
        Ok(read)
    }
}

/// Writer adaptor produced by [`Xorcism::writer`].
pub struct XorcismWriter<'a, W> {
    inner: W,
    munger: Xorcism<'a>,
}

impl<W: Write> Write for XorcismWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut scratch = [0u8; 1024];
        let mut written = 0;

        while written < buf.len() {
            let end = (written + scratch.len()).min(buf.len());
            let chunk = &buf[written..end];
            scratch[..chunk.len()].copy_from_slice(chunk);
            self.munger.munge_in_place(&mut scratch[..chunk.len()]);
            self.inner.write_all(&scratch[..chunk.len()])?;
            written = end;
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
''',
        },
    },
    {
        "name": "edit",
        "arguments": {
            "path": "src/lib.rs",
            "old": r'''    pub fn munge<Data>(&mut self, data: Data) -> impl Iterator<Item = u8> + '_
    where
        Data: IntoIterator,
        Data::Item: Borrow<u8>,
    {''',
            "new": r'''    pub fn munge<'b, Data>(&'b mut self, data: Data) -> impl Iterator<Item = u8> + 'b
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
