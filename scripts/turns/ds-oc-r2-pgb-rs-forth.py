THOUGHT = r'''The scaffolding is minimal. I'll implement a token-list interpreter: each user word is stored as a resolved op list referencing prior definitions by arena index, so redefinition keeps old semantics and nested definitions don't get textually expanded (which is what makes naive implementations blow up on alloc-attack). Then I'll verify with my own tests.'''

CONTENT = ""

CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": r'''use std::collections::HashMap;

pub type Value = i32;
pub type Result = std::result::Result<(), Error>;

#[derive(Clone, Copy)]
enum Op {
    Push(Value),
    Add,
    Sub,
    Mul,
    Div,
    Dup,
    Drop,
    Swap,
    Over,
    Call(usize),
}

pub struct Forth {
    stack: Vec<Value>,
    defs: Vec<Vec<Op>>,
    words: HashMap<String, usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    DivisionByZero,
    StackUnderflow,
    UnknownWord,
    InvalidWord,
}

fn parse_number(token: &str) -> Option<Value> {
    let digits = token.strip_prefix('-').unwrap_or(token);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse::<Value>().ok()
}

fn compile(token: &str, words: &HashMap<String, usize>) -> Option<Op> {
    if let Some(n) = parse_number(token) {
        return Some(Op::Push(n));
    }
    let key = token.to_ascii_uppercase();
    match key.as_str() {
        "+" => Some(Op::Add),
        "-" => Some(Op::Sub),
        "*" => Some(Op::Mul),
        "/" => Some(Op::Div),
        "DUP" => Some(Op::Dup),
        "DROP" => Some(Op::Drop),
        "SWAP" => Some(Op::Swap),
        "OVER" => Some(Op::Over),
        _ => words.get(&key).copied().map(Op::Call),
    }
}

impl Forth {
    pub fn new() -> Forth {
        Forth {
            stack: Vec::new(),
            defs: Vec::new(),
            words: HashMap::new(),
        }
    }

    pub fn stack(&self) -> &[Value] {
        &self.stack
    }

    pub fn eval(&mut self, input: &str) -> Result {
        let tokens: Vec<&str> = input.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i];
            i += 1;
            if token == ":" {
                let name = *tokens.get(i).ok_or(Error::InvalidWord)?;
                i += 1;
                if name == ";" || name == ":" || parse_number(name).is_some() {
                    return Err(Error::InvalidWord);
                }
                let mut body = Vec::new();
                let mut terminated = false;
                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;
                    if t == ";" {
                        terminated = true;
                        break;
                    }
                    if t == ":" {
                        return Err(Error::InvalidWord);
                    }
                    body.push(compile(t, &self.words).ok_or(Error::UnknownWord)?);
                }
                if !terminated {
                    return Err(Error::InvalidWord);
                }
                self.defs.push(body);
                let id = self.defs.len() - 1;
                self.words.insert(name.to_ascii_uppercase(), id);
            } else if token == ";" {
                return Err(Error::InvalidWord);
            } else {
                let op = compile(token, &self.words).ok_or(Error::UnknownWord)?;
                self.exec(op)?;
            }
        }
        Ok(())
    }

    fn exec(&mut self, op: Op) -> Result {
        match op {
            Op::Push(v) => self.stack.push(v),
            Op::Add => {
                let (a, b) = self.pop2()?;
                self.stack.push(a.wrapping_add(b));
            }
            Op::Sub => {
                let (a, b) = self.pop2()?;
                self.stack.push(a.wrapping_sub(b));
            }
            Op::Mul => {
                let (a, b) = self.pop2()?;
                self.stack.push(a.wrapping_mul(b));
            }
            Op::Div => {
                if self.stack.len() < 2 {
                    return Err(Error::StackUnderflow);
                }
                let (a, b) = (self.stack[self.stack.len() - 2], self.stack[self.stack.len() - 1]);
                if b == 0 {
                    return Err(Error::DivisionByZero);
                }
                self.stack.truncate(self.stack.len() - 2);
                self.stack.push(a.wrapping_div(b));
            }
            Op::Dup => {
                let top = *self.stack.last().ok_or(Error::StackUnderflow)?;
                self.stack.push(top);
            }
            Op::Drop => {
                self.stack.pop().ok_or(Error::StackUnderflow)?;
            }
            Op::Swap => {
                let n = self.stack.len();
                if n < 2 {
                    return Err(Error::StackUnderflow);
                }
                self.stack.swap(n - 1, n - 2);
            }
            Op::Over => {
                let n = self.stack.len();
                if n < 2 {
                    return Err(Error::StackUnderflow);
                }
                self.stack.push(self.stack[n - 2]);
            }
            Op::Call(id) => self.run(id)?,
        }
        Ok(())
    }

    fn pop2(&mut self) -> std::result::Result<(Value, Value), Error> {
        let n = self.stack.len();
        if n < 2 {
            return Err(Error::StackUnderflow);
        }
        let a = self.stack[n - 2];
        let b = self.stack[n - 1];
        self.stack.truncate(n - 2);
        Ok((a, b))
    }

    fn run(&mut self, def: usize) -> Result {
        let mut frames: Vec<(usize, usize)> = vec![(def, 0)];
        while let Some(&(id, pc)) = frames.last() {
            if pc >= self.defs[id].len() {
                frames.pop();
                continue;
            }
            frames.last_mut().unwrap().1 += 1;
            let op = self.defs[id][pc];
            if let Op::Call(next) = op {
                frames.push((next, 0));
            } else {
                self.exec(op)?;
            }
        }
        Ok(())
    }
}
'''}},
    {"name": "write", "arguments": {"filePath": "tests/local_check.rs", "content": r'''use forth::{Error, Forth, Value};

fn run(input: &str) -> std::result::Result<Vec<Value>, Error> {
    let mut f = Forth::new();
    f.eval(input)?;
    Ok(f.stack().to_vec())
}

#[test]
fn basics() {
    assert_eq!(run("").unwrap(), vec![]);
    assert_eq!(run("1 2 3").unwrap(), vec![1, 2, 3]);
    assert_eq!(run("1 2 +").unwrap(), vec![3]);
    assert_eq!(run("5 3 -").unwrap(), vec![2]);
    assert_eq!(run("2 4 *").unwrap(), vec![8]);
    assert_eq!(run("8 2 /").unwrap(), vec![4]);
    assert_eq!(run("1 2 swap").unwrap(), vec![2, 1]);
    assert_eq!(run("1 2 over").unwrap(), vec![1, 2, 1]);
    assert_eq!(run("1 dup").unwrap(), vec![1, 1]);
    assert_eq!(run("1 2 drop").unwrap(), vec![1]);
}

#[test]
fn user_words() {
    assert_eq!(run(": dup 5 ; dup").unwrap(), vec![5]);
    assert_eq!(run(": foo 1 2 + ; foo foo").unwrap(), vec![3, 3]);
    assert_eq!(run(": foo 1 ; : bar foo foo ; bar").unwrap(), vec![1, 1]);
    assert_eq!(run(": FOO 1 ; foo").unwrap(), vec![1]);
    assert_eq!(run(": foo 1 ; : foo 2 ; foo").unwrap(), vec![2]);
    assert_eq!(run(": foo 1 ; : foo foo 2 ; foo").unwrap(), vec![1, 2]);
    assert_eq!(run(": foo ; foo").unwrap(), vec![]);
    assert_eq!(run(": foo 1 2"), Err(Error::InvalidWord));
}

#[test]
fn errors() {
    assert_eq!(run("1 +"), Err(Error::StackUnderflow));
    assert_eq!(run("drop"), Err(Error::StackUnderflow));
    assert_eq!(run("1 2 / 0 /"), Err(Error::DivisionByZero));
    assert_eq!(run("nope"), Err(Error::UnknownWord));
    assert_eq!(run(": 1 2 ;"), Err(Error::InvalidWord));
    assert_eq!(run(": foo bar ;"), Err(Error::UnknownWord));
}

#[test]
fn alloc_attack() {
    let mut f = Forth::new();
    assert!(f.eval(": w0 1 ;").is_ok());
    for i in 1..32 {
        assert!(f.eval(&format!(": w{i} w{} w{} ;", i - 1, i - 1)).is_ok());
    }
    assert_eq!(f.stack(), &[1]);
}
'''}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -40"}},
]
