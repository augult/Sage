THOUGHT = r'''Implement Forth by compiling each definition to a token body that captures the
definitions visible at definition time, so redefining a word later does not change
earlier bodies and ": foo foo ..." refers to the previous foo. Store bodies behind
Rc so chained definitions share tokens instead of expanding exponentially.'''

CONTENT = ""

LIB = r'''use std::collections::HashMap;
use std::rc::Rc;

pub type Value = i32;
pub type Result = std::result::Result<(), Error>;

pub struct Forth {
    stack: Vec<Value>,
    words: HashMap<String, Rc<Vec<Token>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Builtin {
    Add,
    Sub,
    Mul,
    Div,
    Dup,
    Drop,
    Swap,
    Over,
}

#[derive(Clone, Debug)]
enum Token {
    Number(Value),
    Builtin(Builtin),
    Call(Rc<Vec<Token>>),
    /// A word unknown when the body was compiled; looked up when executed.
    Word(String),
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
    token.parse().ok()
}

fn builtin(name: &str) -> Option<Builtin> {
    Some(match name {
        "+" => Builtin::Add,
        "-" => Builtin::Sub,
        "*" => Builtin::Mul,
        "/" => Builtin::Div,
        "dup" => Builtin::Dup,
        "drop" => Builtin::Drop,
        "swap" => Builtin::Swap,
        "over" => Builtin::Over,
        _ => return None,
    })
}

impl Forth {
    pub fn new() -> Forth {
        Forth {
            stack: Vec::new(),
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
            match token {
                ":" => {
                    let name = tokens.get(i).ok_or(Error::InvalidWord)?;
                    if parse_number(name).is_some() || *name == ":" || *name == ";" {
                        return Err(Error::InvalidWord);
                    }
                    let name = name.to_ascii_lowercase();
                    i += 1;
                    let mut body = Vec::new();
                    let mut closed = false;
                    while i < tokens.len() {
                        let t = tokens[i];
                        i += 1;
                        if t == ";" {
                            closed = true;
                            break;
                        }
                        body.push(self.compile(t));
                    }
                    if !closed {
                        return Err(Error::InvalidWord);
                    }
                    self.words.insert(name, Rc::new(body));
                }
                ";" => return Err(Error::InvalidWord),
                _ => {
                    let compiled = self.compile(token);
                    self.execute(&compiled)?;
                }
            }
        }
        Ok(())
    }

    /// Bind a token to what it means right now, so later redefinitions do not
    /// affect already compiled bodies and sharing bodies stays cheap.
    fn compile(&self, token: &str) -> Token {
        if let Some(number) = parse_number(token) {
            return Token::Number(number);
        }
        let name = token.to_ascii_lowercase();
        if let Some(definition) = self.words.get(&name) {
            Token::Call(Rc::clone(definition))
        } else if let Some(b) = builtin(&name) {
            Token::Builtin(b)
        } else {
            Token::Word(name)
        }
    }

    fn execute(&mut self, token: &Token) -> Result {
        match token {
            Token::Number(n) => {
                self.stack.push(*n);
                Ok(())
            }
            Token::Builtin(b) => self.apply(*b),
            Token::Call(body) => {
                for t in body.iter() {
                    self.execute(t)?;
                }
                Ok(())
            }
            Token::Word(name) => {
                if let Some(definition) = self.words.get(name).cloned() {
                    self.execute(&Token::Call(definition))
                } else if let Some(b) = builtin(name) {
                    self.apply(b)
                } else {
                    Err(Error::UnknownWord)
                }
            }
        }
    }

    fn pop(&mut self) -> std::result::Result<Value, Error> {
        self.stack.pop().ok_or(Error::StackUnderflow)
    }

    fn binary(&mut self, op: fn(Value, Value) -> Value) -> Result {
        let rhs = self.pop()?;
        let lhs = self.pop()?;
        self.stack.push(op(lhs, rhs));
        Ok(())
    }

    fn apply(&mut self, builtin: Builtin) -> Result {
        match builtin {
            Builtin::Add => self.binary(Value::wrapping_add),
            Builtin::Sub => self.binary(Value::wrapping_sub),
            Builtin::Mul => self.binary(Value::wrapping_mul),
            Builtin::Div => {
                let divisor = self.pop()?;
                let dividend = self.pop()?;
                if divisor == 0 {
                    return Err(Error::DivisionByZero);
                }
                self.stack.push(dividend.wrapping_div(divisor));
                Ok(())
            }
            Builtin::Dup => {
                let top = *self.stack.last().ok_or(Error::StackUnderflow)?;
                self.stack.push(top);
                Ok(())
            }
            Builtin::Drop => {
                self.pop()?;
                Ok(())
            }
            Builtin::Swap => {
                let len = self.stack.len();
                if len < 2 {
                    return Err(Error::StackUnderflow);
                }
                self.stack.swap(len - 1, len - 2);
                Ok(())
            }
            Builtin::Over => {
                let len = self.stack.len();
                if len < 2 {
                    return Err(Error::StackUnderflow);
                }
                let value = self.stack[len - 2];
                self.stack.push(value);
                Ok(())
            }
        }
    }
}
'''

CALLS = [
    {"name": "write", "arguments": {"path": "src/lib.rs", "content": LIB, "overwrite": True}},
    {"name": "shell", "arguments": {
        "cmd": "cargo build --quiet && cp /tmp/forth_tests.rs tests/zz_sage_check.rs && cargo test --quiet --test zz_sage_check -- --include-ignored",
        "expect_exit": "0",
        "expect_has": "test result: ok"}},
]
