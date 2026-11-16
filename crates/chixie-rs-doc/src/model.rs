//! Document model: one text flow of characters with stable ids and run attributes, split into
//! paragraphs at `'\n'`. Every change goes through [`Command`], which returns its exact inverse,
//! so undo/redo never copies the whole document.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Character formatting. `None` = inherit the document default (today's single font and size).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attrs {
    /// Font role or font name (`rFonts` in DOCX).
    pub font: Option<String>,
    /// Size in points.
    pub size_pt: Option<f32>,
    /// Pen colour, RGB.
    pub colour: Option<[u8; 3]>,
    /// Raised (+) or lowered (−) baseline, mm.
    pub v_offset: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Paragraph formatting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Para {
    pub align: Align,
    /// First-line indent, mm; `None` = the handwriting default.
    pub first_indent: Option<f32>,
}

/// One character as stored: the glyph, its stable id (keys the handwriting randomness) and its
/// formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct Char {
    pub ch: char,
    pub id: u64,
    pub attrs: Attrs,
}

/// The flow. Invariants: `chars`, `ids` and `attrs` have one entry per character; `paras` has one
/// entry per paragraph (`'\n'` count + 1); every `attrs` entry indexes `styles`.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    chars: Vec<char>,
    ids: Vec<u64>,
    attrs: Vec<u32>,
    /// Interned attribute sets; index 0 is the default.
    styles: Vec<Attrs>,
    paras: Vec<Para>,
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self { chars: Vec::new(), ids: Vec::new(), attrs: Vec::new(), styles: vec![Attrs::default()], paras: vec![Para::default()], next_id: 0 }
    }
}

/// An edit with its inverse. `Splice` covers insert and delete (and is its own inverse with the
/// two sides swapped); paragraph records travel with the newlines they belong to.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Replace the chars at `at..at + removed.len()` with `inserted`. `removed_paras` are the
    /// records of the paragraphs that started after each removed `'\n'`; `inserted_paras` the
    /// same for the inserted newlines.
    Splice { at: usize, removed: Vec<Char>, inserted: Vec<Char>, removed_paras: Vec<Para>, inserted_paras: Vec<Para> },
    /// Set the formatting of `at..at + attrs.len()`; `old` is filled in by `apply`.
    SetAttrs { at: usize, attrs: Vec<Attrs> },
    /// Set the records of paragraphs `first..first + paras.len()`.
    SetParas { first: usize, paras: Vec<Para> },
}

impl Document {
    /// Plain text with fresh ids `0..n`.
    pub fn from_text(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let ids = (0..chars.len() as u64).collect();
        Self::from_parts(chars, ids)
    }

    /// Text with saved ids (renumbered if they don't match one to one or repeat).
    pub fn from_parts(chars: Vec<char>, ids: Vec<u64>) -> Self {
        let unique = { let mut s = ids.clone(); s.sort_unstable(); s.windows(2).all(|w| w[0] != w[1]) };
        let ids = if ids.len() == chars.len() && unique { ids } else { (0..chars.len() as u64).collect() };
        let next_id = ids.iter().max().map_or(0, |m| m + 1);
        let paras = vec![Para::default(); chars.iter().filter(|&&c| c == '\n').count() + 1];
        Self { attrs: vec![0; chars.len()], chars, ids, styles: vec![Attrs::default()], paras, next_id }
    }

    /// Build from loaded characters and paragraph records. `None` if the parts don't fit
    /// together (paragraph count, repeated ids).
    pub fn from_loaded(chars: Vec<Char>, paras: Vec<Para>) -> Option<Self> {
        let mut d = Self { paras, ..Self::default() };
        for c in chars {
            let a = d.intern(&c.attrs);
            d.chars.push(c.ch);
            d.ids.push(c.id);
            d.attrs.push(a);
            d.next_id = d.next_id.max(c.id + 1);
        }
        d.is_consistent().then_some(d)
    }

    pub fn chars(&self) -> &[char] {
        &self.chars
    }
    pub fn ids(&self) -> &[u64] {
        &self.ids
    }
    pub fn len(&self) -> usize {
        self.chars.len()
    }
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }
    pub fn attrs(&self, i: usize) -> &Attrs {
        &self.styles[self.attrs[i] as usize]
    }
    pub fn paras(&self) -> &[Para] {
        &self.paras
    }
    pub fn char(&self, i: usize) -> Char {
        Char { ch: self.chars[i], id: self.ids[i], attrs: self.attrs(i).clone() }
    }
    /// Paragraph index of character position `i` (a `'\n'` belongs to the paragraph it ends).
    pub fn para_of(&self, i: usize) -> usize {
        self.chars[..i.min(self.len())].iter().filter(|&&c| c == '\n').count()
    }
    /// Maximal ranges of equal formatting that don't cross a newline-free boundary of attrs.
    pub fn runs(&self) -> Vec<(Range<usize>, &Attrs)> {
        let mut out: Vec<(Range<usize>, &Attrs)> = Vec::new();
        for i in 0..self.len() {
            match out.last_mut() {
                Some((r, _)) if self.attrs[r.start] == self.attrs[i] => r.end = i + 1,
                _ => out.push((i..i + 1, self.attrs(i))),
            }
        }
        out
    }

    /// Fresh characters for `text` with formatting `attrs` (ids are reserved now).
    pub fn new_chars(&mut self, text: &str, attrs: &Attrs) -> Vec<Char> {
        text.chars()
            .map(|ch| {
                self.next_id += 1;
                Char { ch, id: self.next_id - 1, attrs: attrs.clone() }
            })
            .collect()
    }

    /// Formatting new text at `i` takes: the char before it, else the one after, else default.
    pub fn attrs_at(&self, i: usize) -> Attrs {
        let j = if i > 0 { i - 1 } else { 0 };
        if j < self.len() { self.attrs(j).clone() } else { Attrs::default() }
    }

    /// A splice replacing `range` with `inserted`; new paragraphs copy the record of the one
    /// they split from (as in Word).
    pub fn splice_cmd(&self, range: Range<usize>, inserted: Vec<Char>) -> Command {
        let removed: Vec<Char> = range.clone().map(|i| self.char(i)).collect();
        let p = self.para_of(range.start);
        let gone = removed.iter().filter(|c| c.ch == '\n').count();
        let removed_paras = self.paras[p + 1..p + 1 + gone].to_vec();
        let born = inserted.iter().filter(|c| c.ch == '\n').count();
        let inserted_paras = vec![self.paras[p].clone(); born];
        Command::Splice { at: range.start, removed, inserted, removed_paras, inserted_paras }
    }

    fn intern(&mut self, a: &Attrs) -> u32 {
        match self.styles.iter().position(|s| s == a) {
            Some(i) => i as u32,
            None => {
                self.styles.push(a.clone());
                (self.styles.len() - 1) as u32
            }
        }
    }

    /// Apply `cmd`; returns its inverse. Panics on a command that doesn't fit the document (a
    /// programming error: commands are built from this document).
    pub fn apply(&mut self, cmd: Command) -> Command {
        match cmd {
            Command::Splice { at, removed, inserted, removed_paras, inserted_paras } => {
                let end = at + removed.len();
                debug_assert!(removed.iter().zip(at..end).all(|(c, i)| c.ch == self.chars[i] && c.id == self.ids[i]));
                let p = self.para_of(at);
                let attrs: Vec<u32> = inserted.iter().map(|c| self.intern(&c.attrs)).collect();
                self.chars.splice(at..end, inserted.iter().map(|c| c.ch));
                self.ids.splice(at..end, inserted.iter().map(|c| c.id));
                self.attrs.splice(at..end, attrs);
                self.paras.splice(p + 1..p + 1 + removed_paras.len(), inserted_paras.iter().cloned());
                if let Some(m) = inserted.iter().map(|c| c.id).max() {
                    self.next_id = self.next_id.max(m + 1);
                }
                Command::Splice { at, removed: inserted, inserted: removed, removed_paras: inserted_paras, inserted_paras: removed_paras }
            }
            Command::SetAttrs { at, attrs } => {
                let old = (at..at + attrs.len()).map(|i| self.attrs(i).clone()).collect();
                for (i, a) in (at..).zip(&attrs) {
                    self.attrs[i] = self.intern(a);
                }
                Command::SetAttrs { at, attrs: old }
            }
            Command::SetParas { first, paras } => {
                let old = self.paras[first..first + paras.len()].to_vec();
                self.paras.splice(first..first + paras.len(), paras);
                Command::SetParas { first, paras: old }
            }
        }
    }

    /// Structural equality ignoring the interning table and the id counter.
    pub fn same_content(&self, other: &Self) -> bool {
        self.chars == other.chars
            && self.ids == other.ids
            && self.paras == other.paras
            && (0..self.len()).all(|i| self.attrs(i) == other.attrs(i))
    }

    /// Check the invariants (tests, and after loading a file).
    pub fn is_consistent(&self) -> bool {
        let n = self.chars.len();
        let mut ids = self.ids.clone();
        ids.sort_unstable();
        self.ids.len() == n
            && self.attrs.len() == n
            && self.attrs.iter().all(|&a| (a as usize) < self.styles.len())
            && self.paras.len() == self.chars.iter().filter(|&&c| c == '\n').count() + 1
            && ids.windows(2).all(|w| w[0] != w[1])
            && self.ids.iter().all(|&id| id < self.next_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_splits_and_merges_paragraphs() {
        let mut d = Document::from_text("ab\ncd");
        d.apply(Command::SetParas { first: 1, paras: vec![Para { align: Align::Center, first_indent: None }] });
        let new = d.new_chars("X\nY", &Attrs::default());
        let inv = d.apply(d.splice_cmd(4..4, new));
        assert_eq!(d.text(), "ab\ncX\nYd");
        assert_eq!(d.paras().len(), 3);
        assert_eq!(d.paras()[2].align, Align::Center, "the split-off paragraph keeps the format");
        assert!(d.is_consistent());
        d.apply(inv);
        assert_eq!(d.text(), "ab\ncd");
        assert_eq!(d.paras().len(), 2);
        // Deleting a newline merges: the first paragraph's format stays.
        let before = d.clone();
        let inv = d.apply(d.splice_cmd(2..3, vec![]));
        assert_eq!((d.text().as_str(), d.paras().len(), d.paras()[0].align), ("abcd", 1, Align::Left));
        d.apply(inv);
        assert!(d.same_content(&before));
    }

    #[test]
    fn set_attrs_and_runs() {
        let mut d = Document::from_text("abcd");
        let red = Attrs { colour: Some([255, 0, 0]), ..Default::default() };
        let inv = d.apply(Command::SetAttrs { at: 1, attrs: vec![red.clone(); 2] });
        let runs: Vec<_> = d.runs().into_iter().map(|(r, a)| (r, a.colour)).collect();
        assert_eq!(runs, vec![(0..1, None), (1..3, Some([255, 0, 0])), (3..4, None)]);
        d.apply(inv);
        assert_eq!(d.runs().len(), 1);
    }

    #[test]
    fn duplicate_or_short_ids_are_renumbered() {
        assert_eq!(Document::from_parts(vec!['a', 'b'], vec![5, 5]).ids(), &[0, 1]);
        assert_eq!(Document::from_parts(vec!['a', 'b'], vec![5]).ids(), &[0, 1]);
        assert_eq!(Document::from_parts(vec!['a', 'b'], vec![7, 3]).ids(), &[7, 3]);
    }
}
