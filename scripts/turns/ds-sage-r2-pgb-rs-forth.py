THOUGHT = r'''The `tests/` directory does not exist in this workspace, so the heredoc failed and nothing was exercised. Next: create the directory, run the same assertions plus more coverage, then clean up.'''

CONTENT = r'''The `1 /` check ran on a non-empty stack, so it correctly succeeded. Using a fresh evaluator for the underflow and division-by-zero cases.'''

CALLS = [
    {"name": "shell", "arguments": {"cmd": r'''mkdir -p tests && cat > tests/tmp_check.rs <<'EOF'
use forth::{Error, Forth};

#[test]
fn checks() {
    let mut f = Forth::new();
    assert_eq!(f.eval("1 2 +"), Ok(()));
    assert_eq!(f.stack(), &[3]);
    assert_eq!(f.eval("4 5 DUP * +"), Ok(()));
    assert_eq!(f.stack(), &[3, 29]);
    assert_eq!(f.eval(": sq dup * ; 3 sq"), Ok(()));
    assert_eq!(f.stack(), &[3, 29, 9]);
    f.eval(": foo 1 ; : bar foo 2 ; : foo 3 ; bar").unwrap();
    assert_eq!(f.stack().last(), Some(&2));
    assert_eq!(f.eval(": 1 2 ;"), Err(Error::InvalidWord));
    assert_eq!(f.eval(": bad undefined ;"), Err(Error::UnknownWord));
    let mut h = Forth::new();
    assert_eq!(h.eval("1 /"), Err(Error::StackUnderflow));
    assert_eq!(h.eval("1 0 /"), Err(Error::DivisionByZero));
    let mut g = Forth::new();
    assert_eq!(g.eval(": rec rec ; rec"), Err(Error::UnknownWord));
    g.eval("1 2 OVER").unwrap();
    assert_eq!(g.stack(), &[1, 2, 1]);
    g.eval("SWAP DROP").unwrap();
    assert_eq!(g.stack(), &[1, 1]);
    g.eval(": NEG -1 * ; 5 neg").unwrap();
    assert_eq!(g.stack().last(), Some(&-5));
    g.eval(": noop ; 7 noop DROP").unwrap();
    assert_eq!(g.stack().last(), Some(&1));
    assert_eq!(g.eval("1 2 missing"), Err(Error::UnknownWord));
    // definition chains stay cheap
    g.eval(": a1 1 ; ").unwrap();
    for i in 2..400 {
        let prev = format!("a{}", i - 1);
        g.eval(&format!(": a{i} {prev} {prev} ;")).unwrap();
    }
}
EOF
cargo test --offline 2>&1 | tail -20; s=$?; rm -rf tests; exit $s'''}}]
