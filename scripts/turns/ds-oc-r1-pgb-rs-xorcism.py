THOUGHT = "Hold the key as a borrowed byte slice plus a position cursor. A private next_byte helper yields the next key byte, so munge_in_place and munge share the state machine; munge wraps the input iterator in a lazy map, avoiding allocation. Under the io feature, wrap Read/Write with small adaptors that reuse the same cursor."

CONTENT = "Writing the implementation."

CODE = r'''use std::borrow::Borrow;

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
    pub fn new<Key: ?Sized + AsRef<[u8]>>(key: &'a Key) -> Xorcism<'a> {
        Xorcism {
            key: key.as_ref(),
            position: 0,
        }
    }

    /// The next byte of the key, repeating it as often as needed.
    fn next_byte(&mut self) -> u8 {
        if self.key.is_empty() {
            return 0;
        }
        let byte = self.key[self.position % self.key.len()];
        self.position = self.position.wrapping_add(1);
        byte
    }

    /// XOR each byte of the input buffer with a byte from the key.
    ///
    /// Note that this is stateful: repeated calls are likely to produce different results,
    /// even with identical inputs.
    pub fn munge_in_place(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            *byte ^= self.next_byte();
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
        data.into_iter().map(move |byte| *byte.borrow() ^ self.next_byte())
    }
}

#[cfg(feature = "io")]
use std::io::{self, Read, Write};

#[cfg(feature = "io")]
impl<'a> Xorcism<'a> {
    /// Convert this munger into a `Read` adaptor.
    pub fn reader<R: Read + 'a>(self, reader: R) -> impl Read + 'a {
        XorReader {
            inner: reader,
            munger: self,
        }
    }

    /// Convert this munger into a `Write` adaptor.
    pub fn writer<W: Write + 'a>(self, writer: W) -> impl Write + 'a {
        XorWriter {
            inner: writer,
            munger: self,
        }
    }
}

#[cfg(feature = "io")]
struct XorReader<'a, R> {
    inner: R,
    munger: Xorcism<'a>,
}

#[cfg(feature = "io")]
struct XorWriter<'a, W> {
    inner: W,
    munger: Xorcism<'a>,
}

#[cfg(feature = "io")]
impl<'a, R: Read> Read for XorReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.munger.munge_in_place(&mut buf[..read]);
        Ok(read)
    }
}

#[cfg(feature = "io")]
impl<'a, W: Write> Write for XorWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let munged: Vec<u8> = buf.iter().copied().map(|b| b ^ self.munger.next_byte()).collect();
        self.inner.write(&munged)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
'''

CALLS = [
    {"name": "bash", "arguments": {"command": "cargo build 2>&1 | tail -25; echo '=== with io ==='; cargo build --features io 2>&1 | tail -25"}},
]
