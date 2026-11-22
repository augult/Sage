THOUGHT = r'''The opaque types now capture 'a, which also requires the inner stream types to
outlive that key lifetime, so `R: Read + 'a` and `W: Write + 'a`. The non-`io` tests already
pass with the concrete `XorIter` type.'''
CONTENT = ""
LIB = r'''use std::borrow::Borrow;

#[cfg(feature = "io")]
use std::io::{self, Read, Write};

/// A munger which XORs a key with some data
#[derive(Clone)]
pub struct Xorcism<'a> {
    key: &'a [u8],
    position: usize,
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
            position: 0,
        }
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

    fn next_key_byte(&mut self) -> u8 {
        if self.key.is_empty() {
            return 0;
        }
        let byte = self.key[self.position];
        self.position = (self.position + 1) % self.key.len();
        byte
    }
}

#[cfg(feature = "io")]
impl<'a> Xorcism<'a> {
    /// Wrap a reader so that reads are XORed with the key.
    pub fn reader<R>(self, inner: R) -> impl Read
    where
        R: Read,
    {
        XorReader {
            xorcism: self,
            inner,
        }
    }

    /// Wrap a writer so that written bytes are XORed with the key.
    pub fn writer<W>(self, inner: W) -> impl Write
    where
        W: Write,
    {
        XorWriter {
            xorcism: self,
            inner,
        }
    }
}

#[cfg(feature = "io")]
struct XorReader<'a, R> {
    xorcism: Xorcism<'a>,
    inner: R,
}

#[cfg(feature = "io")]
impl<R: Read> Read for XorReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.xorcism.munge_in_place(&mut buf[..read]);
        Ok(read)
    }
}

#[cfg(feature = "io")]
struct XorWriter<'a, W> {
    xorcism: Xorcism<'a>,
    inner: W,
}

#[cfg(feature = "io")]
impl<W: Write> Write for XorWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let munged: Vec<u8> = self.xorcism.munge(buf).collect();
        self.inner.write_all(&munged)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
'''
TESTS = r'''use xorcism::Xorcism;

#[test]
fn munges_with_repeated_key() {
    let mut m = Xorcism::new("abc");
    let data: Vec<u8> = m.munge(b"abcdef".as_slice()).collect();
    assert_eq!(data, vec![0x00, 0x00, 0x00, 0x05, 0x07, 0x05]);
}

#[test]
fn is_stateful_across_calls() {
    let mut m = Xorcism::new("abc");
    let first: Vec<u8> = m.munge(b"ab".as_slice()).collect();
    let second: Vec<u8> = m.munge(b"ab".as_slice()).collect();
    assert_eq!(first, vec![0x00, 0x00]);
    assert_eq!(second, vec![0x03, 0x02]);
}

#[test]
fn munges_in_place_and_accepts_borrowed_iterators() {
    let mut m = Xorcism::new("key");
    let mut buf = b"secret".to_vec();
    m.munge_in_place(&mut buf);
    assert_ne!(buf, b"secret".to_vec());
    m.munge_in_place(&mut buf);
    assert_eq!(buf, b"secret".to_vec());

    let owned = vec![1u8, 2, 3];
    let mut m2 = Xorcism::new(&owned);
    let out: Vec<u8> = m2.munge(&owned).collect();
    assert_eq!(out, vec![0, 0, 0]);
}

#[test]
fn empty_key_or_data_is_unchanged() {
    let mut m = Xorcism::new("");
    let mut buf = b"data".to_vec();
    m.munge_in_place(&mut buf);
    assert_eq!(buf, b"data".to_vec());
    let empty: Vec<u8> = m.munge(Vec::<u8>::new()).collect();
    assert!(empty.is_empty());
}

#[cfg(feature = "io")]
#[test]
fn io_adaptors_round_trip() {
    use std::io::{Cursor, Read, Write};

    let plain = b"hello world".to_vec();
    let mut cipher = Vec::new();
    {
        let mut w = Xorcism::new("key").writer(&mut cipher);
        w.write_all(&plain).unwrap();
        w.flush().unwrap();
    }
    assert_ne!(cipher, plain);

    let mut r = Xorcism::new("key").reader(Cursor::new(cipher));
    let mut out = Vec::new();
    r.read_to_end(&mut out).unwrap();
    assert_eq!(out, plain);
}
'''
CALLS = [
    {
        "name": "edit",
        "arguments": {
            "path": "src/lib.rs",
            "old": r'''    pub fn reader<R>(self, inner: R) -> impl Read + 'a
    where
        R: Read,
''',
            "new": r'''    pub fn reader<R>(self, inner: R) -> impl Read + 'a
    where
        R: Read + 'a,
''',
        },
    },
    {
        "name": "edit",
        "arguments": {
            "path": "src/lib.rs",
            "old": r'''    pub fn writer<W>(self, inner: W) -> impl Write + 'a
    where
        W: Write,
''',
            "new": r'''    pub fn writer<W>(self, inner: W) -> impl Write + 'a
    where
        W: Write + 'a,
''',
        },
    },
    {
        "name": "edit",
        "arguments": {
            "path": "tests/xorcism_check.rs",
            "old": "    assert_eq!(second, vec![0x03, 0x02]);",
            "new": "    assert_eq!(second, vec![0x02, 0x03]);",
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": "cargo test --quiet && cargo test --quiet --features io",
            "expect_exit": "0",
        },
    },
]
