//! A safe reader for Python pickles: the format of ChiXie3's `.cx` templates and `.cset` device
//! parameter files.
//!
//! Only opcodes are interpreted. A class reference (`GLOBAL`, `STACK_GLOBAL`) is recorded by name
//! and never imported or called; `REDUCE`, `NEWOBJ` and `BUILD` yield a [`Value::Object`] holding
//! the class name and state. A hostile pickle therefore comes back as data, not as code.
//!
//! Parsing works on an arena so a memoised container can be filled in after it is referenced, then
//! the arena is copied into an owned tree. [`Limits`] bound both steps.

use std::collections::HashMap;

use crate::{Error, Result};

/// A parsed pickle value, owned.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    /// Entries in file order. A repeated key keeps both; [`Value::get`] returns the last.
    Dict(Vec<(Value, Value)>),
    /// An instance of `class` (dotted `module.Name`, never resolved) with its `BUILD` state, or an
    /// empty dict when the pickle set none.
    Object { class: String, state: Box<Value> },
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The value under the string key `key` of a dict (the last one, as Python's `dict` keeps).
    pub fn get(&self, key: &str) -> Option<&Value> {
        let Value::Dict(items) = self else { return None };
        items.iter().rev().find(|(k, _)| k.as_str() == Some(key)).map(|(_, v)| v)
    }
}

/// Caps on the work a pickle can cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Opcodes run, plus nodes copied out. A shared container is copied once per reference, so
    /// this also caps the blow-up from pickles that reuse one large container many times.
    pub max_ops: usize,
    /// Container nesting depth. Also what stops a self-referencing container.
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_ops: 1 << 20, max_depth: 64 }
    }
}

/// Parse one pickle with [`Limits::default`]. Nothing in it is ever executed.
pub fn parse(bytes: &[u8]) -> Result<Value> {
    parse_with(bytes, Limits::default())
}

/// Parse one pickle under `limits`.
pub fn parse_with(bytes: &[u8], limits: Limits) -> Result<Value> {
    let mut vm = Vm { nodes: Vec::new(), stack: Vec::new(), marks: Vec::new(), memo: HashMap::new(), memo_next: 0, ops: 0, limits };
    let root = vm.run(bytes)?;
    copy_out(&vm.nodes, root, 0, &mut vm.ops, limits)
}

fn bad(msg: &str) -> Error {
    Error::Config(format!("pickle: {msg}"))
}

fn spend(ops: &mut usize, limits: Limits) -> Result<()> {
    *ops += 1;
    if *ops > limits.max_ops { Err(bad("too many operations")) } else { Ok(()) }
}

/// An arena slot. Children are slot indices, so a memo reference is just an index.
enum Node {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<usize>),
    Tuple(Vec<usize>),
    Dict(Vec<(usize, usize)>),
    /// A class reference. Only ever used as the callable of `REDUCE` / `NEWOBJ`.
    Class(String),
    Object { class: String, state: usize },
}

struct Vm {
    nodes: Vec<Node>,
    stack: Vec<usize>,
    marks: Vec<usize>,
    memo: HashMap<usize, usize>,
    memo_next: usize,
    ops: usize,
    limits: Limits,
}

impl Vm {
    fn run(&mut self, b: &[u8]) -> Result<usize> {
        let mut r = Reader { b, i: 0 };
        loop {
            spend(&mut self.ops, self.limits)?;
            let op = r.u8()?;
            match op {
                0x80 => {
                    r.take(1)?; // PROTO
                }
                0x95 => {
                    r.take(8)?; // FRAME
                }
                b'.' => return self.pop(),
                b'(' => self.marks.push(self.stack.len()),
                b'}' => self.push(Node::Dict(Vec::new())),
                b']' => self.push(Node::List(Vec::new())),
                b')' => self.push(Node::Tuple(Vec::new())),
                b'N' => self.push(Node::None),
                0x88 => self.push(Node::Bool(true)),
                0x89 => self.push(Node::Bool(false)),
                b'K' => {
                    let v = r.u8()?;
                    self.push(Node::Int(v.into()))
                }
                b'M' => {
                    let v = u16::from_le_bytes(r.array()?);
                    self.push(Node::Int(v.into()))
                }
                b'J' => {
                    let v = i32::from_le_bytes(r.array()?);
                    self.push(Node::Int(v.into()))
                }
                0x8a => {
                    // LONG1: little-endian two's complement, n bytes
                    let n = r.u8()? as usize;
                    if n > 8 {
                        return Err(bad("integer wider than 64 bits"));
                    }
                    let bytes = r.take(n)?;
                    let mut v: i64 = 0;
                    for (k, &byte) in bytes.iter().enumerate() {
                        v |= i64::from(byte) << (8 * k);
                    }
                    if n > 0 && n < 8 && bytes[n - 1] & 0x80 != 0 {
                        v -= 1i64 << (8 * n);
                    }
                    self.push(Node::Int(v))
                }
                b'G' => {
                    let v = f64::from_be_bytes(r.array()?);
                    self.push(Node::Float(v))
                }
                0x8c => {
                    let n = r.u8()? as usize;
                    let s = r.utf8(n)?;
                    self.push(Node::Str(s))
                }
                b'X' => {
                    let n = u32::from_le_bytes(r.array()?) as usize;
                    let s = r.utf8(n)?;
                    self.push(Node::Str(s))
                }
                0x8d => {
                    let n = u64::from_le_bytes(r.array()?) as usize;
                    let s = r.utf8(n)?;
                    self.push(Node::Str(s))
                }
                0x94 => {
                    // MEMOIZE
                    let t = self.top()?;
                    self.memo.insert(self.memo_next, t);
                    self.memo_next += 1;
                }
                b'q' => {
                    let k = r.u8()? as usize;
                    self.memo_put(k)?
                }
                b'r' => {
                    let k = u32::from_le_bytes(r.array()?) as usize;
                    self.memo_put(k)?
                }
                b'h' => {
                    let k = r.u8()? as usize;
                    self.memo_get(k)?
                }
                b'j' => {
                    let k = u32::from_le_bytes(r.array()?) as usize;
                    self.memo_get(k)?
                }
                0x93 => {
                    // STACK_GLOBAL
                    let name = self.pop()?;
                    let module = self.pop()?;
                    let (Some(module), Some(name)) = (self.str_at(module), self.str_at(name)) else {
                        return Err(bad("GLOBAL names are not strings"));
                    };
                    let full = format!("{module}.{name}");
                    self.push(Node::Class(full))
                }
                b'c' => {
                    let module = r.line()?;
                    let name = r.line()?;
                    self.push(Node::Class(format!("{module}.{name}")))
                }
                0x81 | b'R' => {
                    // NEWOBJ (cls, args) / REDUCE (callable, args): the args are dropped, the call is
                    // recorded as an object of that class and never made.
                    let _args = self.pop()?;
                    let cls = self.pop()?;
                    let class = match &self.nodes[cls] {
                        Node::Class(c) => c.clone(),
                        _ => return Err(bad("REDUCE / NEWOBJ without a class")),
                    };
                    let state = self.add(Node::Dict(Vec::new()));
                    self.push(Node::Object { class, state })
                }
                b'b' => {
                    // BUILD: the state (a dict, or (dict, slot state)) goes on the object below it.
                    let mut st = self.pop()?;
                    if let Node::Tuple(t) = &self.nodes[st]
                        && let Some(&first) = t.first()
                    {
                        st = first;
                    }
                    let obj = self.top()?;
                    match &mut self.nodes[obj] {
                        Node::Object { state, .. } => *state = st,
                        _ => return Err(bad("BUILD on a non-object")),
                    }
                }
                b's' => {
                    let v = self.pop()?;
                    let k = self.pop()?;
                    let d = self.top()?;
                    self.set_items(d, &[(k, v)])?
                }
                b'u' => {
                    let kv = self.pop_mark()?;
                    if kv.len() % 2 != 0 {
                        return Err(bad("SETITEMS with an odd count"));
                    }
                    let pairs: Vec<(usize, usize)> = kv.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                    let d = self.top()?;
                    self.set_items(d, &pairs)?
                }
                b'a' | b'e' => {
                    let items = if op == b'a' { vec![self.pop()?] } else { self.pop_mark()? };
                    let l = self.top()?;
                    match &mut self.nodes[l] {
                        Node::List(xs) => xs.extend(items),
                        _ => return Err(bad("APPEND on a non-list")),
                    }
                }
                b't' => {
                    let xs = self.pop_mark()?;
                    self.push(Node::Tuple(xs))
                }
                0x85..=0x87 => {
                    let n = (op - 0x84) as usize;
                    if self.stack.len() < n {
                        return Err(bad("stack underflow"));
                    }
                    let xs = self.stack.split_off(self.stack.len() - n);
                    self.push(Node::Tuple(xs))
                }
                other => return Err(bad(&format!("unsupported pickle opcode 0x{other:02x}"))),
            }
        }
    }

    fn add(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn push(&mut self, n: Node) {
        let i = self.add(n);
        self.stack.push(i);
    }

    fn pop(&mut self) -> Result<usize> {
        self.stack.pop().ok_or_else(|| bad("stack underflow"))
    }

    fn top(&self) -> Result<usize> {
        self.stack.last().copied().ok_or_else(|| bad("empty stack"))
    }

    fn pop_mark(&mut self) -> Result<Vec<usize>> {
        let m = self.marks.pop().ok_or_else(|| bad("no mark"))?;
        if m > self.stack.len() {
            return Err(bad("mark past stack"));
        }
        Ok(self.stack.split_off(m))
    }

    fn str_at(&self, i: usize) -> Option<&str> {
        match &self.nodes[i] {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    fn set_items(&mut self, dict: usize, pairs: &[(usize, usize)]) -> Result<()> {
        let Node::Dict(items) = &mut self.nodes[dict] else { return Err(bad("SETITEM on a non-dict")) };
        items.extend_from_slice(pairs);
        Ok(())
    }

    fn memo_put(&mut self, k: usize) -> Result<()> {
        let t = self.top()?;
        self.memo.insert(k, t);
        Ok(())
    }

    fn memo_get(&mut self, k: usize) -> Result<()> {
        let i = *self.memo.get(&k).ok_or_else(|| bad("bad memo reference"))?;
        self.stack.push(i);
        Ok(())
    }
}

/// Copy arena node `i` into an owned tree, charging every copied node against `ops`.
fn copy_out(nodes: &[Node], i: usize, depth: usize, ops: &mut usize, limits: Limits) -> Result<Value> {
    if depth > limits.max_depth {
        return Err(bad("nested too deeply"));
    }
    spend(ops, limits)?;
    Ok(match &nodes[i] {
        Node::None => Value::None,
        Node::Bool(b) => Value::Bool(*b),
        Node::Int(v) => Value::Int(*v),
        Node::Float(v) => Value::Float(*v),
        Node::Str(s) => Value::Str(s.clone()),
        Node::List(xs) => Value::List(copy_all(nodes, xs, depth, ops, limits)?),
        Node::Tuple(xs) => Value::Tuple(copy_all(nodes, xs, depth, ops, limits)?),
        Node::Dict(kv) => {
            let mut out = Vec::with_capacity(kv.len());
            for &(k, v) in kv {
                let key = copy_out(nodes, k, depth + 1, ops, limits)?;
                out.push((key, copy_out(nodes, v, depth + 1, ops, limits)?));
            }
            Value::Dict(out)
        }
        Node::Class(_) => return Err(bad("class reference used as data")),
        Node::Object { class, state } => Value::Object { class: class.clone(), state: Box::new(copy_out(nodes, *state, depth + 1, ops, limits)?) },
    })
}

fn copy_all(nodes: &[Node], xs: &[usize], depth: usize, ops: &mut usize, limits: Limits) -> Result<Vec<Value>> {
    xs.iter().map(|&x| copy_out(nodes, x, depth + 1, ops, limits)).collect()
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| bad("truncated"))?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("length checked"))
    }
    fn utf8(&mut self, n: usize) -> Result<String> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad("invalid UTF-8"))
    }
    fn line(&mut self) -> Result<String> {
        let rest = &self.b[self.i..];
        let n = rest.iter().position(|&c| c == b'\n').ok_or_else(|| bad("unterminated GLOBAL"))?;
        let s = self.utf8(n)?;
        self.i += 1;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scalars_and_lists() {
        // [None, True, False, 7, -2, 1.5, 'hi']
        let p = b"\x80\x04](N\x88\x89K\x07J\xfe\xff\xff\xffG?\xf8\x00\x00\x00\x00\x00\x00\x8c\x02hie.";
        assert_eq!(
            parse(p).unwrap(),
            Value::List(vec![Value::None, Value::Bool(true), Value::Bool(false), Value::Int(7), Value::Int(-2), Value::Float(1.5), Value::Str("hi".into())])
        );
    }

    #[test]
    fn memo_reference_copies_the_container() {
        // outer = [x, x] where x = [] is memoised once.
        let p = b"\x80\x04]]q\x00ah\x00a.";
        let empty = Value::List(vec![]);
        assert_eq!(parse(p).unwrap(), Value::List(vec![empty.clone(), empty]));
    }

    #[test]
    fn dict_get_finds_string_keys() {
        // {'a': 1}
        let p = b"\x80\x04}(X\x01\x00\x00\x00aK\x01u.";
        let v = parse(p).unwrap();
        assert_eq!(v.get("a"), Some(&Value::Int(1)));
        assert_eq!(v.get("b"), None);
    }

    #[test]
    fn class_reference_is_data_not_code() {
        // os.system('echo pwned') as a GLOBAL + REDUCE: recorded as an object, never called.
        let p = b"\x80\x02cos\nsystem\n\x8c\x0aecho pwned\x85R.";
        assert_eq!(parse(p).unwrap(), Value::Object { class: "os.system".into(), state: Box::new(Value::Dict(vec![])) });
    }

    #[test]
    fn bare_class_as_data_is_refused() {
        // [os.system]
        let p = b"\x80\x02](cos\nsystem\ne.";
        assert!(parse(p).is_err());
    }

    #[test]
    fn nesting_depth_is_limited() {
        let nested = |n: usize| {
            let mut p = b"\x80\x04".to_vec();
            p.extend(std::iter::repeat_n(b']', n));
            p.extend(std::iter::repeat_n(b'a', n - 1));
            p.push(b'.');
            p
        };
        assert!(parse(&nested(10)).is_ok());
        assert!(parse(&nested(100)).is_err());
    }

    #[test]
    fn operation_count_is_limited() {
        // [1, 1, ... 20 times]
        let mut p = b"\x80\x04](".to_vec();
        p.extend(std::iter::repeat_n([b'K', 1], 20).flatten());
        p.extend_from_slice(b"e.");
        assert!(parse(&p).is_ok());
        assert!(parse_with(&p, Limits { max_ops: 10, max_depth: 64 }).is_err());
    }

    #[test]
    fn shared_references_cannot_blow_up() {
        // L0 = []; Lk = [L(k-1), L(k-1)]: 2^20 leaves once copied out.
        let mut p = b"\x80\x04]q\x00".to_vec();
        for k in 1..=20u8 {
            p.extend_from_slice(&[b']', b'(', b'h', k - 1, b'h', k - 1, b'e', b'q', k]);
        }
        p.push(b'.');
        assert!(parse(&p).is_err());
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse(b"not a pickle").is_err());
        assert!(parse(&[0x80, 0x04, 0x95]).is_err());
        assert!(parse(b"").is_err());
    }
}
