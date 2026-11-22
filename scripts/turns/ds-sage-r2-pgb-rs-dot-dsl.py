THOUGHT = r'''Wrong path: `graph_items` is a child of `graph`, not of the crate root, so the import
must go through `self`. Then rerun the same suite.'''

CONTENT = ""

CALLS = [
    {"name": "edit", "arguments": {
        "path": "src/lib.rs",
        "old": "    use super::graph_items::{edge::Edge, node::Node};",
        "new": "    use self::graph_items::{edge::Edge, node::Node};"}},
    {"name": "shell", "arguments": {
        "cmd": r'''mkdir -p tests && cat > tests/tmp_dot_check.rs <<'EOF'
use dot_dsl::graph::graph_items::edge::Edge;
use dot_dsl::graph::graph_items::node::Node;
use dot_dsl::graph::Graph;

#[test]
fn empty_graph() {
    let graph = Graph::new();
    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(graph.attrs.is_empty());
}

#[test]
fn nodes_and_edges() {
    let nodes = vec![
        Node::new("a").with_attrs(&[("color", "green")]),
        Node::new("b"),
    ];
    let edges = vec![Edge::new("a", "b").with_attrs(&[("color", "red")])];
    let graph = Graph::new()
        .with_attrs(&[("bgcolor", "yellow")])
        .with_nodes(&nodes)
        .with_edges(&edges);

    assert_eq!(graph.attrs, vec![("bgcolor", "yellow")]);
    assert_eq!(graph.nodes[0].name, "a");
    assert_eq!(graph.nodes[0].attrs, vec![("color", "green")]);
    assert_eq!(graph.nodes[1].attrs, Vec::<(&str, &str)>::new());
    assert_eq!(graph.edges[0].nodes, ("a", "b"));
    assert_eq!(graph.edges[0].attrs, vec![("color", "red")]);
}

#[test]
fn lookups() {
    let graph = Graph::new()
        .with_nodes(&[Node::new("a"), Node::new("b")])
        .with_edges(&[Edge::new("a", "b")]);
    assert_eq!(graph.get_node("a").unwrap().name, "a");
    assert!(graph.get_node("c").is_none());
    assert_eq!(graph.get_edge("b", "a"), None);
    assert_eq!(graph.get_edge("a", "b").unwrap().nodes, ("a", "b"));
}
EOF
cargo test --quiet; status=$?; rm -rf tests; exit $status''',
        "expect_exit": "0"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented the dot-DSL builder API in `src/lib.rs`: `graph::Graph` with public `attrs`, `nodes` and `edges`, owner-style `new`/`with_attrs`/`with_nodes`/`with_edges` builders, and `get_node`/`get_edge` lookups; `graph::graph_items::node::Node` and `::edge::Edge` with `new`, `with_attrs` and public fields, all borrowing `&str` and deriving Clone/Debug/PartialEq/Eq. Verified with `cargo test` covering an empty graph, nodes and edges carrying attributes, graph-level attributes, and both lookups including misses.",
        "unverified": ""}},
]
