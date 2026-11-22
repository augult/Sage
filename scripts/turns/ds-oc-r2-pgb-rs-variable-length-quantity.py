THOUGHT = """Two bugs: the continuation bit landed on the wrong byte after reversing, and decoding accumulated least-significant-first instead of most-significant-first. Fixing both."""
CONTENT = """Fixing the continuation-bit placement and the decode accumulation order, then re-running the tests."""
CALLS = [
    {"name": "bash", "arguments": {"command": r'''cat > src/lib.rs <<'EOF'
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    IncompleteNumber,
}

/// Convert a list of numbers to a stream of bytes encoded with variable length encoding.
pub fn to_bytes(values: &[u32]) -> Vec<u8> {
    let mut encoded = Vec::new();

    for &value in values {
        let mut groups = vec![(value & 0x7f) as u8];
        let mut rest = value >> 7;
        while rest > 0 {
            groups.push((rest & 0x7f) as u8);
            rest >>= 7;
        }

        // All groups except the least significant one are continued.
        for group in &mut groups[1..] {
            *group |= 0x80;
        }

        encoded.extend(groups.iter().rev());
    }

    encoded
}

/// Given a stream of bytes, extract all numbers which are encoded in there.
pub fn from_bytes(bytes: &[u8]) -> Result<Vec<u32>, Error> {
    let mut decoded = Vec::new();
    let mut value: u64 = 0;
    let mut pending = false;

    for &byte in bytes {
        pending = true;
        value = (value << 7) | u64::from(byte & 0x7f);

        if byte & 0x80 == 0 {
            decoded.push(value as u32);
            value = 0;
            pending = false;
        }
    }

    if pending {
        return Err(Error::IncompleteNumber);
    }

    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_single_values() {
        assert_eq!(to_bytes(&[0x0]), vec![0x00]);
        assert_eq!(to_bytes(&[0x40]), vec![0x40]);
        assert_eq!(to_bytes(&[0x7f]), vec![0x7f]);
        assert_eq!(to_bytes(&[0x80]), vec![0x81, 0x00]);
        assert_eq!(to_bytes(&[0x2000]), vec![0xc0, 0x00]);
        assert_eq!(to_bytes(&[0x3fff]), vec![0xff, 0x7f]);
        assert_eq!(to_bytes(&[0x4000]), vec![0x81, 0x80, 0x00]);
        assert_eq!(to_bytes(&[0x1fffff]), vec![0xff, 0xff, 0x7f]);
        assert_eq!(to_bytes(&[0x200000]), vec![0x81, 0x80, 0x80, 0x00]);
        assert_eq!(to_bytes(&[0xffffffff]), vec![0x8f, 0xff, 0xff, 0xff, 0x7f]);
    }

    #[test]
    fn encodes_sequences() {
        assert_eq!(to_bytes(&[0x7f, 0x4000]), vec![0x7f, 0x81, 0x80, 0x00]);
        assert_eq!(to_bytes(&[]), Vec::<u8>::new());
    }

    #[test]
    fn decodes_values() {
        assert_eq!(from_bytes(&[0x7f]), Ok(vec![0x7f]));
        assert_eq!(from_bytes(&[0xc0, 0x00]), Ok(vec![0x2000]));
        assert_eq!(from_bytes(&[0x81, 0x80, 0x00]), Ok(vec![0x4000]));
        assert_eq!(
            from_bytes(&[0x8f, 0xff, 0xff, 0xff, 0x7f]),
            Ok(vec![0xffffffff])
        );
        assert_eq!(from_bytes(&[]), Ok(vec![]));
    }

    #[test]
    fn rejects_incomplete_numbers() {
        assert_eq!(from_bytes(&[0x80]), Err(Error::IncompleteNumber));
        assert_eq!(from_bytes(&[0x7f, 0x81]), Err(Error::IncompleteNumber));
    }
}
EOF
cargo test 2>&1 | tail -20'''}},
]
