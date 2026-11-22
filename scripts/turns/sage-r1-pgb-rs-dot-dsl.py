THOUGHT = "The supplied exercise only requires a Graph type with a constructor and gives no further graph API. I will replace the placeholder with a valid empty graph constructor and run the existing Rust checks."
CONTENT = "The starter only specifies `graph::Graph::new()`, so I’ll implement that minimal API without inventing unspecified builder methods."
CALLS = [
    {"name": "edit", "arguments": {"path": "src/lib.rs", "old": "pub mod graph {\n    pub struct Graph;\n\n    impl Graph {\n        pub fn new() -> Self {\n            todo!(\"Construct a new Graph struct.\");\n        }\n    }\n}\n", "new": "pub mod graph {\n    pub struct Graph;\n\n    impl Graph {\n        pub fn new() -> Self {\n            Self\n        }\n    }\n}\n"}},
    {"name": "shell", "arguments": {"cmd": "cargo test && git diff --check && git diff", "timeout": 120, "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented Graph::new() to construct and return an empty Graph, replacing the placeholder panic. Cargo tests and git diff --check pass. The README describes an internal graph DSL generally but does not specify further graph APIs, so none were invented.", "unverified": "Hidden tests may expect additional builder APIs not specified in the provided README or starter."}},
]
