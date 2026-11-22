THOUGHT = """Design: Xorcism holds the key slice plus a running position, so the key cycle is stateful across calls. `next_key_byte` is the shared helper both `munge_in_place` and `munge` use. `munge` copies the `&'a [u8]` out of self so only the position is mutably borrowed, then maps with a borrow-agnostic `Borrow<u8>` item. Bonus reader/writer wrap Read/Write with the same munging."""
CONTENT = """Implementing the struct, both munging methods, and the io stream adaptors."""
CALLS = [
    {"name": "bash", "arguments": {"command": r'''cat > src/lib.rs <<'EOF'
use std::borrow::Borrow;
use std::io::{Read, Write};

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

    /// The next byte of the key, cycling back to the start when exhausted.
    fn next_key_byte(&mut self) -> u8 {
        if self.key.is_empty() {
            return 0;
        }
        let byte = self.key[self.pos];
        self.pos = (self.pos + 1) % self.key.len();
        byte
    }

    /// XOR each byte of the input buffer with a byte from the key.
    ///
    /// Note that this is stateful: repeated calls are likely to produce different results,
    /// even with identical inputs.
    pub fn munge_in_place(&mut self, data: &mut [u8]) {
        for byte in data {
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
        let key = self.key;
        let pos = &mut self.pos;

        data.into_iter().map(move |data_byte| {
            let key_byte = if key.is_empty() {
                0
            } else {
                let byte = key[*pos % key.len()];
                *pos = (*pos + 1) % key.len();
                byte
            };
            data_byte.borrow() ^ key_byte
        })
    }

    /// Turn this munger into a reader which munges data as it is read.
    pub fn reader<R: Read>(self, inner: R) -> XorReader<'a, R> {
        XorReader {
            xorcism: self,
            inner,
        }
    }

    /// Turn this munger into a writer which munges data as it is written.
    pub fn writer<W: Write>(self, inner: W) -> XorWriter<'a, W> {
        XorWriter {
            xorcism: self,
            inner,
        }
    }
}

pub struct XorReader<'a, R> {
    xorcism: Xorcism<'a>,
    inner: R,
}

impl<R: Read> Read for XorReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.xorcism.munge_in_place(&mut buf[..read]);
        Ok(read)
    }
}

pub struct XorWriter<'a, W> {
    xorcism: Xorcism<'a>,
    inner: W,
}

impl<W: Write> Write for XorWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let munged: Vec<u8> = self.xorcism.munge(buf).collect();
        self.inner.write(&munged)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reports_key_cycling() {
        let mut munger = Xorcism::new(&[1u8, 2, 3]);
        let result: Vec<u8> = munger.munge(&[0, 0, 0, 0, 0]);
        assert_eq!(result, vec![1, 2, 3, 1, 2]);
    }

    #[test]
    fn xor_is_symmetric() {
        let key = b"abcdefghijklmnop";
        let data = b"the quick brown fox jumps over the lazy dog";

        let encrypted: Vec<u8> = Xorcism::new(key).munge(data).collect();
        assert_ne!(encrypted, data.to_vec());

        let decrypted: Vec<u8> = Xorcism::new(key).munge(&encrypted).collect();
        assert_eq!(decrypted, data.to_vec());
    }

    #[test]
    fn munge_in_place_and_statefulness() {
        let mut munger = Xorcism::new(&[1u8, 2, 3]);
        let mut first = [0u8; 3];
        let mut second = [0u8; 3];
        munger.munge_in_place(&mut first);
        munger.munge_in_place(&mut second);
        assert_eq!(first, [1, 2, 3]);
        assert_eq!(second, [1, 2, 3]);

        // The position carries across calls rather than resetting.
        let mut odd = Xorcism::new(&[1u8, 2, 3]);
        let mut one = [0u8; 1];
        odd.munge_in_place(&mut one);
        let rest: Vec<u8> = odd.munge(&[0u8, 0]).collect();
        assert_eq!(one, [1]);
        assert_eq!(rest, vec![2, 3]);
    }

    #[test]
    fn reader_round_trips() {
        let key = b"secret";
        let plain = b"hello, xorcism";
        let cipher: Vec<u8> = Xorcism::new(key).reader(&plain[..]).bytes_all();
        assert_ne!(&cipher, plain);
        let back: Vec<u8> = Xorcism::new(key).reader(&cipher[..]).bytes_all();
        assert_eq!(&back, plain);
    }

    #[test]
    fn writer_munges_output() {
        let key = b"secret";
        let mut sink = Vec::new();
        {
            let mut writer = Xorcism::new(key).writer(&mut sink);
            writer.write_all(b"hello").unwrap();
        }
        let mut expected = vec![0u8; 5];
        Xorcism::new(key).munge_in_place(&mut expected);
        let mut expected: Vec<u8> = b"hello".iter().zip(expected).map(|(a, b)| a ^ b).collect();
        assert_eq!(sink, expected);
    }

    trait ReadBytes {
        fn bytes_all(self) -> Vec<u8>;
    }

    impl<R: Read> ReadBytes for R {
        fn bytes_all(mut self) -> Vec<u8> {
            let mut out = Vec::new();
            self.read_to_end(&mut out).unwrap();
            out
        }
    }

    #[test]
    fn munge_accepts_owned_and_borrowed() {
        let mut munger = Xorcism::new(&[1u8]);
        let owned: Vec<u8> = munger.munge(vec![0u8, 0]).collect();
        assert_eq!(owned, vec![1, 1]);
        let borrowed: Vec<u8> = munger.munge(&[0u8, 0][..]).collect();
        assert_eq!(borrowed, vec![1, 1]);
    }

    #[test]
    fn empty_key_is_identity() {
        let mut munger = Xorcism::new(&[] as &[u8]);
        let result: Vec<u8> = munger.munge(&[5u8, 6, 7]).collect();
        assert_eq!(result, vec![5, 6, 7]);
    }
}
EOF
cargo test 2>&1 | tail -30'''}},
]
