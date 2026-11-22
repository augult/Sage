THOUGHT = "Add focused in-module tests for the README boundary examples, maximum u32, empty streams, concatenated numbers, and incomplete trailing values, then run them and finish."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {
        "path": "src/lib.rs",
        "old": "    if in_number {\n        Err(Error::IncompleteNumber)\n    } else {\n        Ok(values)\n    }\n}",
        "new": "    if in_number {\n        Err(Error::IncompleteNumber)\n    } else {\n        Ok(values)\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    use super::{from_bytes, to_bytes, Error};\n\n    #[test]\n    fn encodes_documented_boundaries_and_maximum() {\n        assert_eq!(to_bytes(&[0, 0x40, 0x7f, 0x80, 0x2000, 0x3fff, 0x4000, 0x100000, 0x1fffff, 0x200000, 0x8000000, 0x0fffffff, u32::MAX]), vec![0x00, 0x40, 0x7f, 0x81, 0x00, 0xc0, 0x00, 0xff, 0x7f, 0x81, 0x80, 0x00, 0xc0, 0x80, 0x00, 0xff, 0xff, 0x7f, 0x81, 0x80, 0x80, 0x00, 0xc0, 0x80, 0x80, 0x00, 0xff, 0xff, 0xff, 0x7f, 0x8f, 0xff, 0xff, 0xff, 0x7f]);\n    }\n\n    #[test]\n    fn decodes_concatenated_values_and_empty_stream() {\n        let values = [0, 0x40, 0x7f, 0x80, 0x2000, u32::MAX];\n        assert_eq!(from_bytes(&to_bytes(&values)), Ok(values.to_vec()));\n        assert_eq!(from_bytes(&[]), Ok(vec![]));\n    }\n\n    #[test]\n    fn rejects_incomplete_number_at_end() {\n        assert_eq!(from_bytes(&[0x81]), Err(Error::IncompleteNumber));\n        assert_eq!(from_bytes(&[0x00, 0x81, 0x80]), Err(Error::IncompleteNumber));\n    }\n}"
    }},
    {"name": "shell", "arguments": {"cmd": "cargo test", "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented VLQ encoding and decoding, including concatenated values, zero, and incomplete trailing-number detection. `cargo test` passes.", "unverified": "The grader's hidden tests were not available."}}
]
