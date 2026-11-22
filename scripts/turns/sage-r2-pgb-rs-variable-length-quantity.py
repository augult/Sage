THOUGHT = "Encode each u32 from its most-significant 7-bit group; decode bytes by shifting the accumulator and emit at each byte whose continuation bit is clear."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {
        "path": "src/lib.rs",
        "old": "    todo!(\"Convert the values {values:?} to a list of bytes\")",
        "new": "    let mut encoded = Vec::new();\n    for &value in values {\n        let mut groups = [0u8; 5];\n        let mut number = value;\n        let mut index = groups.len();\n        loop {\n            index -= 1;\n            groups[index] = (number & 0x7f) as u8;\n            number >>= 7;\n            if number == 0 {\n                break;\n            }\n        }\n        for position in index..groups.len() {\n            let continuation = if position + 1 < groups.len() { 0x80 } else { 0 };\n            encoded.push(groups[position] | continuation);\n        }\n    }\n    encoded"
    }},
    {"name": "edit", "arguments": {
        "path": "src/lib.rs",
        "old": "    todo!(\"Convert the list of bytes {bytes:?} to a list of numbers\")",
        "new": "    let mut values = Vec::new();\n    let mut current = 0u32;\n    let mut in_number = false;\n    for &byte in bytes {\n        current = (current << 7) | u32::from(byte & 0x7f);\n        in_number = byte & 0x80 != 0;\n        if !in_number {\n            values.push(current);\n            current = 0;\n        }\n    }\n    if in_number {\n        Err(Error::IncompleteNumber)\n    } else {\n        Ok(values)\n    }"
    }},
    {"name": "shell", "arguments": {"cmd": "cargo test", "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented VLQ encoding and decoding, including concatenated values, zero, and incomplete trailing-number detection. `cargo test` passes.", "unverified": "The grader's hidden tests were not available."}}
]
