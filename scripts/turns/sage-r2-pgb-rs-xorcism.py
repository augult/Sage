THOUGHT = "Add focused tests for iterator/in-place key cycling and the optional reader/writer paths, then run both feature configurations."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"path": "src/lib.rs", "lines": "40-56"}},
    {"name": "edit", "arguments": {
        "path": "src/lib.rs",
        "old": "    fn flush(&mut self) -> std::io::Result<()> {\n        self.writer.flush()\n    }\n}",
        "new": "    fn flush(&mut self) -> std::io::Result<()> {\n        self.writer.flush()\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    use super::Xorcism;\n\n    #[test]\n    fn iterator_and_slice_methods_share_key_position() {\n        let key = b\"abc\";\n        let mut xorcism = Xorcism::new(&key);\n        assert_eq!(xorcism.munge([0u8, 0]).collect::<Vec<_>>(), vec![b'a', b'b']);\n        let mut data = [0u8, 0];\n        xorcism.munge_in_place(&mut data);\n        assert_eq!(data, [b'c', b'a']);\n    }\n\n    #[cfg(feature = \"io\")]\n    #[test]\n    fn reader_and_writer_transform_streams() {\n        use std::io::{Cursor, Read, Write};\n        let key = b\"k\";\n        let mut reader = Xorcism::new(&key).reader(Cursor::new(vec![b'a', b'b']));\n        let mut decoded = Vec::new();\n        reader.read_to_end(&mut decoded).unwrap();\n        assert_eq!(decoded, vec![b'a' ^ b'k', b'b' ^ b'k']);\n\n        let mut encoded = Vec::new();\n        {\n            let mut writer = Xorcism::new(&key).writer(&mut encoded);\n            writer.write_all(b\"ab\").unwrap();\n            writer.flush().unwrap();\n        }\n        assert_eq!(encoded, vec![b'a' ^ b'k', b'b' ^ b'k']);\n    }\n}"
    }},
    {"name": "shell", "arguments": {"cmd": "cargo test && cargo test --features io", "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented borrowed-key XOR munging for slices and iterators, plus optional streaming reader and writer adaptors. Both default and `io` test commands pass.", "unverified": "Hidden grader tests were unavailable."}}
]
