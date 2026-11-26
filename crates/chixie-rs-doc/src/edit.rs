//! Editable document behind the WYSIWYG page editor: a [`Document`] plus caret, selection, IME
//! composition and undo history.
//!
//! Each character keeps the id it was inserted with, and `layout::layout_ids` keys its
//! handwriting randomness by that id, so editing never reshuffles the rest of the page.
//! Positions are char indices (`0..=len`); the IME bridge converts to/from UTF-16 with
//! [`EditDoc::to_utf16`] / [`EditDoc::from_utf16`]. Undo stores the inverse [`Command`]s of each
//! step, grouped like Word: a typing (or deleting) burst and an IME composition are one step.

use std::ops::Range;

use chixie_rs_core::handwriting::is_cjk;
use chixie_rs_core::layout::CELL_BREAK;

use crate::model::{Tuning, Align, Attrs, CharStyle, Command, Document, Para};

mod objects;
pub use objects::{TABLE_WIDTH, TableSpan};

/// One undo (or redo) step: commands to apply in reverse order, and the selection to restore.
#[derive(Debug, Clone)]
struct Step {
    cmds: Vec<Command>,
    caret: usize,
    anchor: Option<usize>,
}

/// What the last edit was, to coalesce consecutive typing into one undo step.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LastEdit {
    None,
    Typing,
    Deleting,
}

#[derive(Debug, Clone)]
pub struct EditDoc {
    doc: Document,
    caret: usize,
    /// Selection anchor; the selection is between anchor and caret.
    anchor: Option<usize>,
    /// IME composition range (char indices), part of the text while composing.
    marked: Option<Range<usize>>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    last: LastEdit,
}

const UNDO_LIMIT: usize = 500;

impl Default for EditDoc {
    fn default() -> Self {
        Self::from_document(Document::default())
    }
}

impl EditDoc {
    pub fn from_text(text: &str) -> Self {
        Self::from_document(Document::from_text(text))
    }

    /// Restore a saved document (ids must match chars one to one; otherwise they are renumbered).
    pub fn from_parts(chars: Vec<char>, ids: Vec<u64>) -> Self {
        Self::from_document(Document::from_parts(chars, ids))
    }

    pub fn from_document(doc: Document) -> Self {
        let caret = doc.len();
        Self { doc, caret, anchor: None, marked: None, undo: Vec::new(), redo: Vec::new(), last: LastEdit::None }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }
    pub fn chars(&self) -> &[char] {
        self.doc.chars()
    }
    pub fn ids(&self) -> &[u64] {
        self.doc.ids()
    }
    pub fn text(&self) -> String {
        self.doc.text()
    }
    pub fn len(&self) -> usize {
        self.doc.len()
    }
    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }
    pub fn caret(&self) -> usize {
        self.caret
    }
    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// Selected range (sorted); empty at the caret when nothing is selected.
    pub fn selection(&self) -> Range<usize> {
        match self.anchor {
            Some(a) if a != self.caret => a.min(self.caret)..a.max(self.caret),
            _ => self.caret..self.caret,
        }
    }
    pub fn has_selection(&self) -> bool {
        !self.selection().is_empty()
    }
    /// True when the caret is at the start of the selection (selection made leftwards).
    pub fn selection_reversed(&self) -> bool {
        self.anchor.is_some_and(|a| a > self.caret)
    }
    pub fn selected_text(&self) -> String {
        self.chars()[self.selection()].iter().collect()
    }

    /// The selected chars' formatting, one entry per char (rich copy).
    pub fn selected_attrs(&self) -> Vec<Attrs> {
        self.selection().map(|i| self.doc.attrs(i).clone()).collect()
    }

    /// Paste `text` over the selection with one `attrs` entry per char (rich paste from our own
    /// copy). Falls back to plain [`insert`](Self::insert) when the counts differ.
    pub fn insert_with_attrs(&mut self, text: &str, attrs: &[Attrs]) {
        if text.chars().count() != attrs.len() {
            self.insert(text);
            return;
        }
        self.checkpoint(LastEdit::None);
        let sel = self.selection();
        let mut new = self.doc.new_chars(text, &Attrs::default());
        for (c, a) in new.iter_mut().zip(attrs) {
            c.attrs = a.clone();
        }
        let n = new.len();
        let cmd = self.doc.splice_cmd(sel.clone(), new);
        self.exec(cmd);
        self.caret = sel.start + n;
        self.anchor = None;
        self.marked = None;
    }

    /// Move the caret; `extend` keeps/starts a selection from the current anchor.
    pub fn move_to(&mut self, i: usize, extend: bool) {
        let i = i.min(self.len());
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = i;
        self.last = LastEdit::None;
    }

    pub fn select_range(&mut self, r: Range<usize>) {
        let (a, b) = (r.start.min(self.len()), r.end.min(self.len()));
        self.anchor = Some(a);
        self.caret = b;
        self.last = LastEdit::None;
    }

    pub fn select_all(&mut self) {
        self.select_range(0..self.len());
    }

    /// Left/right by one char; with a selection and no `extend`, collapse to its edge.
    pub fn left(&mut self, extend: bool) {
        let sel = self.selection();
        let to = if !extend && !sel.is_empty() { sel.start } else { self.caret.saturating_sub(1) };
        self.move_to(to, extend);
    }
    pub fn right(&mut self, extend: bool) {
        let sel = self.selection();
        let to = if !extend && !sel.is_empty() { sel.end } else { (self.caret + 1).min(self.len()) };
        self.move_to(to, extend);
    }

    /// Word boundaries: CJK characters are one word each; runs of other non-space chars are words.
    pub fn word_left(&self, i: usize) -> usize {
        let c = self.chars();
        let mut j = i;
        while j > 0 && c[j - 1].is_whitespace() {
            j -= 1;
        }
        if j > 0 && is_cjk(c[j - 1]) {
            return j - 1;
        }
        while j > 0 && !c[j - 1].is_whitespace() && !is_cjk(c[j - 1]) {
            j -= 1;
        }
        j
    }
    pub fn word_right(&self, i: usize) -> usize {
        let c = self.chars();
        let n = c.len();
        let mut j = i;
        if j < n && is_cjk(c[j]) {
            return j + 1;
        }
        while j < n && !c[j].is_whitespace() && !is_cjk(c[j]) {
            j += 1;
        }
        while j < n && c[j].is_whitespace() && c[j] != '\n' {
            j += 1;
        }
        j
    }
    /// The word around `i` (for double-click).
    pub fn word_at(&self, i: usize) -> Range<usize> {
        let c = self.chars();
        let i = i.min(c.len());
        if i < c.len() && is_cjk(c[i]) {
            return i..i + 1;
        }
        let (mut a, mut b) = (i, i);
        while a > 0 && !c[a - 1].is_whitespace() && !is_cjk(c[a - 1]) {
            a -= 1;
        }
        while b < c.len() && !c[b].is_whitespace() && !is_cjk(c[b]) {
            b += 1;
        }
        a..b
    }

    /// Open a new undo step unless this edit continues the current typing / deleting burst.
    fn checkpoint(&mut self, kind: LastEdit) {
        let coalesce = kind != LastEdit::None && kind == self.last && !self.undo.is_empty();
        if !coalesce {
            self.undo.push(Step { cmds: Vec::new(), caret: self.caret, anchor: self.anchor });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = kind;
    }

    /// Apply `cmd` and record its inverse in the current undo step.
    fn exec(&mut self, cmd: Command) {
        let inv = self.doc.apply(cmd);
        if let Some(step) = self.undo.last_mut() {
            step.cmds.push(inv);
        }
    }

    /// Replace `range` with `text` (new ids, formatting of the text before), leaving the caret
    /// after the insertion.
    fn splice(&mut self, range: Range<usize>, text: &str) -> Range<usize> {
        // Replacing takes the formatting of what it replaces (Word); inserting, of the char before.
        let attrs = if range.is_empty() { self.doc.attrs_at(range.start) } else { self.doc.attrs(range.start).clone() };
        let new = self.doc.new_chars(text, &attrs);
        let n = new.len();
        let start = range.start;
        if !(range.is_empty() && n == 0) {
            let cmd = self.doc.splice_cmd(range, new);
            self.exec(cmd);
        }
        self.caret = start + n;
        self.anchor = None;
        start..start + n
    }

    /// Type or paste `text` over the selection (or at the caret).
    pub fn insert(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 && !text.contains('\n') { LastEdit::Typing } else { LastEdit::None };
        self.checkpoint(kind);
        let sel = self.selection();
        self.splice(sel, text);
        self.marked = None;
    }

    /// Several replacements as one undo step (刷新数据, 批量建页). `edits` must not overlap; they
    /// are applied last-first so each range is still valid. New text takes the formatting before
    /// it; untouched chars keep their ids.
    pub fn replace_many(&mut self, mut edits: Vec<(Range<usize>, String)>) {
        if edits.is_empty() {
            return;
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        self.checkpoint(LastEdit::None);
        let caret = self.caret;
        for (r, text) in edits {
            let r = r.start.min(self.len())..r.end.min(self.len());
            self.splice(r, &text);
        }
        self.caret = caret.min(self.len());
        self.anchor = None;
        self.marked = None;
    }

    /// Table forms (Tab): the caret goes to the start of the next cell. In the last cell a cell
    /// break is added at the end of the text, starting a new cell.
    pub fn next_cell(&mut self) {
        let next = self.doc.chars()[self.caret..].iter().position(|&c| c == CELL_BREAK).map(|k| self.caret + k + 1);
        match next {
            Some(i) => self.move_to(i, false),
            None => {
                self.move_to(self.len(), false);
                self.insert(&CELL_BREAK.to_string());
            }
        }
    }

    /// Shift+Tab: the start of the caret's cell, or of the previous cell when already there.
    pub fn prev_cell(&mut self) {
        let to = {
            let c = self.doc.chars();
            let start = |i: usize| c[..i].iter().rposition(|&x| x == CELL_BREAK).map_or(0, |k| k + 1);
            let here = start(self.caret);
            if self.caret > here { here } else if here > 0 { start(here - 1) } else { 0 }
        };
        self.move_to(to, false);
    }

    /// Delete the selection, or `range` when nothing is selected. Returns true if anything changed.
    fn delete_or(&mut self, range: Range<usize>) -> bool {
        let sel = self.selection();
        let r = if sel.is_empty() { range } else { sel };
        if r.is_empty() {
            return false;
        }
        self.checkpoint(LastEdit::Deleting);
        self.splice(r, "");
        true
    }

    pub fn backspace(&mut self, word: bool) -> bool {
        let from = if word { self.word_left(self.caret) } else { self.caret.saturating_sub(1) };
        self.delete_unguarded(from..self.caret)
    }

    pub fn delete(&mut self, word: bool) -> bool {
        let to = if word { self.word_right(self.caret) } else { (self.caret + 1).min(self.len()) };
        self.delete_unguarded(self.caret..to)
    }

    /// Backspace / Delete without a selection stop at an inline table's cell edges and markers.
    fn delete_unguarded(&mut self, range: Range<usize>) -> bool {
        if !self.has_selection() && range.clone().any(|k| self.guards_table(k)) {
            return false;
        }
        self.delete_or(range)
    }

    /// Change the formatting of the selection (one undo step). No-op without a selection.
    pub fn set_attrs(&mut self, f: impl Fn(&mut Attrs)) {
        let sel = self.selection();
        if sel.is_empty() {
            return;
        }
        self.checkpoint(LastEdit::None);
        let attrs = sel
            .clone()
            .map(|i| {
                let mut a = self.doc.attrs(i).clone();
                f(&mut a);
                a
            })
            .collect();
        self.exec(Command::SetAttrs { at: sel.start, attrs });
    }

    /// One property of the selected chars when they all share it (`None` when they differ: the
    /// ribbon shows it blank). Without a selection, the property new text at the caret takes.
    pub fn common<T: PartialEq>(&self, f: impl Fn(&Attrs) -> T) -> Option<T> {
        let sel = self.selection();
        if sel.is_empty() {
            return Some(f(&self.doc.attrs_at(self.caret)));
        }
        let first = f(self.doc.attrs(sel.start));
        sel.skip(1).all(|i| f(self.doc.attrs(i)) == first).then_some(first)
    }

    /// Turn a format on for the selection, or off when every selected char already has it
    /// (下划线, 删除线, 标题, …; one undo step).
    pub fn toggle_attr(&mut self, has: impl Fn(&Attrs) -> bool, set: impl Fn(&mut Attrs, bool)) {
        let on = self.common(&has) != Some(true);
        self.set_attrs(|a| set(a, on));
    }

    /// 清除格式: the selection's character formats back to the document's (colour, 合并内容
    /// and 字符微调 stay).
    pub fn clear_format(&mut self) {
        self.set_attrs(|a| *a = a.cleared());
    }

    /// 格式刷: give the selection `src`'s character formats.
    pub fn paint_format(&mut self, src: &Attrs) {
        self.set_attrs(|a| *a = a.with_formats_of(src));
    }

    /// Align the paragraphs touched by the selection (or holding the caret).
    pub fn set_align(&mut self, align: Align) {
        self.set_paras(|_, p| p.align = align);
    }

    /// Alignment of the paragraph holding the caret.
    pub fn align(&self) -> Align {
        self.doc.paras()[self.doc.para_of(self.caret)].align
    }

    /// Tab / 首行缩进: indent the touched paragraphs, or un-indent them when all already are.
    /// `width(id)` gives the indent (mm) for a paragraph whose first char has `id`, so the
    /// same paragraph always gets the same random width.
    pub fn toggle_indent(&mut self, width: impl Fn(u64) -> f32) {
        let sel = self.selection();
        let (a, b) = (self.doc.para_of(sel.start), self.doc.para_of(sel.end));
        let on = self.doc.paras()[a..=b].iter().any(|p| p.first_indent.is_none());
        let starts = self.para_starts();
        let ids = self.doc.ids().to_vec();
        // An empty last paragraph has no first char: key it by its index.
        self.set_paras(|k, p| p.first_indent = on.then(|| width(ids.get(starts[k]).copied().unwrap_or(k as u64))));
    }

    /// Whether the paragraph holding the caret has a first-line indent.
    pub fn indented(&self) -> bool {
        self.doc.paras()[self.doc.para_of(self.caret)].first_indent.is_some()
    }

    /// Char index where each paragraph starts.
    fn para_starts(&self) -> Vec<usize> {
        std::iter::once(0).chain(self.doc.chars().iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i + 1)).collect()
    }

    fn set_paras(&mut self, f: impl Fn(usize, &mut Para)) {
        let sel = self.selection();
        let (a, b) = (self.doc.para_of(sel.start), self.doc.para_of(sel.end));
        self.checkpoint(LastEdit::None);
        let paras = self.doc.paras()[a..=b]
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let mut p = *p;
                f(a + k, &mut p);
                p
            })
            .collect();
        self.exec(Command::SetParas { first: a, paras });
    }

    /// 字符微调 per char, for layout.
    pub fn tunings(&self) -> Vec<Option<Tuning>> {
        (0..self.len()).map(|i| self.doc.attrs(i).tuning).collect()
    }

    /// 字符格式 per char for layout; sizes relative to `base_pt` (the document's size).
    pub fn styles(&self, base_pt: f32) -> Vec<Option<CharStyle>> {
        (0..self.len()).map(|i| self.doc.attrs(i).char_style(base_pt)).collect()
    }

    /// (char id, font name) of every char with its own 字体.
    pub fn char_fonts(&self) -> Vec<(u64, &str)> {
        (0..self.len()).filter_map(|i| self.doc.attrs(i).font.as_deref().map(|f| (self.doc.ids()[i], f))).collect()
    }

    /// 字符微调 of the selected chars (one undo step); an identity result clears it.
    pub fn tune(&mut self, f: impl Fn(&mut Tuning)) {
        let sel = self.selection();
        self.tune_where(|i| sel.contains(&i), f);
    }

    /// 应用至同字符: the same change on every char equal to a selected one.
    pub fn tune_same_chars(&mut self, f: impl Fn(&mut Tuning)) {
        let sel = self.selection();
        let wanted: Vec<char> = self.doc.chars()[sel].to_vec();
        let chars = self.doc.chars().to_vec();
        self.tune_where(|i| wanted.contains(&chars[i]), f);
    }

    /// 应用至同字符 with 字体 / 字号 too: `f` on every char equal to a selected one (one undo step).
    pub fn attrs_same_chars(&mut self, f: impl Fn(&mut Attrs)) {
        let sel = self.selection();
        let wanted: Vec<char> = self.doc.chars()[sel].to_vec();
        let chars = self.doc.chars().to_vec();
        self.attrs_where(|i| wanted.contains(&chars[i]), f);
    }

    fn tune_where(&mut self, pick: impl Fn(usize) -> bool, f: impl Fn(&mut Tuning)) {
        self.attrs_where(pick, |a| {
            let mut t = a.tuning.unwrap_or_default();
            f(&mut t);
            a.tuning = (!t.is_identity()).then_some(t);
        });
    }

    fn attrs_where(&mut self, pick: impl Fn(usize) -> bool, f: impl Fn(&mut Attrs)) {
        let hits: Vec<usize> = (0..self.len()).filter(|&i| pick(i)).collect();
        let (Some(&first), Some(&last)) = (hits.first(), hits.last()) else { return };
        self.checkpoint(LastEdit::None);
        let attrs = (first..=last)
            .map(|i| {
                let mut a = self.doc.attrs(i).clone();
                if hits.binary_search(&i).is_ok() {
                    f(&mut a);
                }
                a
            })
            .collect();
        self.exec(Command::SetAttrs { at: first, attrs });
    }

    /// 合并内容 flags per char, for layout (`joins[k]`: char `k + 1` shares char `k`'s cell).
    pub fn joins(&self) -> Vec<bool> {
        (0..self.len()).map(|i| self.doc.attrs(i).combine_next).collect()
    }

    /// 合并内容: put the selected chars in one grid cell, or split them again when they already
    /// are. Errors are ChiXie3's messages.
    pub fn toggle_combine(&mut self) -> Result<(), &'static str> {
        let sel = self.selection();
        if sel.len() < 2 {
            return Err("合并内容只能合并两个以及以上的字符");
        }
        if self.doc.chars()[sel.clone()].contains(&'\n') {
            return Err("作文格模式合并内容只能选中连续文字内容");
        }
        let last = sel.end - 1;
        let on = !(sel.start..last).all(|i| self.doc.attrs(i).combine_next);
        self.checkpoint(LastEdit::None);
        let attrs = sel
            .map(|i| {
                let mut a = self.doc.attrs(i).clone();
                a.combine_next = on && i < last;
                a
            })
            .collect();
        self.exec(Command::SetAttrs { at: self.selection().start, attrs });
        Ok(())
    }

    /// Apply a step's commands in reverse; returns the step that undoes this one.
    fn replay(&mut self, step: Step) -> Step {
        let back = Step { cmds: Vec::new(), caret: self.caret, anchor: self.anchor };
        let cmds = step.cmds.into_iter().rev().map(|c| self.doc.apply(c)).collect();
        self.caret = step.caret.min(self.len());
        self.anchor = step.anchor.map(|a| a.min(self.len()));
        self.marked = None;
        self.last = LastEdit::None;
        Step { cmds, ..back }
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        let r = self.replay(s);
        self.redo.push(r);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        let u = self.replay(s);
        self.undo.push(u);
        true
    }

    // ---- IME composition (platform ranges are UTF-16; see to_utf16/from_utf16) ----

    /// Replace `range` (or the marked text, or the selection) with composition `text`, marking it.
    /// `sel_in_text` is the selection inside the new text (char offsets), if the IME provides one.
    pub fn set_marked(&mut self, range: Option<Range<usize>>, text: &str, sel_in_text: Option<Range<usize>>) {
        let r = range.or_else(|| self.marked.clone()).unwrap_or_else(|| self.selection());
        if self.marked.is_none() {
            self.checkpoint(LastEdit::None);
        }
        let placed = self.splice(r, text);
        self.marked = if placed.is_empty() { None } else { Some(placed.clone()) };
        if let Some(s) = sel_in_text {
            let (a, b) = (placed.start + s.start.min(placed.len()), placed.start + s.end.min(placed.len()));
            self.anchor = (a != b).then_some(a);
            self.caret = b;
        }
    }

    /// Commit: replace `range` (or the marked text, or the selection) with final `text`.
    pub fn commit_text(&mut self, range: Option<Range<usize>>, text: &str) {
        // A composition already opened its undo step when it started.
        let composing = self.marked.is_some();
        let r = range.or_else(|| self.marked.take()).unwrap_or_else(|| self.selection());
        if !composing {
            self.checkpoint(if text.chars().count() == 1 { LastEdit::Typing } else { LastEdit::None });
        }
        self.splice(r, text);
        self.marked = None;
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    /// Char index → UTF-16 offset.
    pub fn to_utf16(&self, i: usize) -> usize {
        self.chars()[..i.min(self.len())].iter().map(|c| c.len_utf16()).sum()
    }

    /// UTF-16 offset → char index (offsets inside a surrogate pair round down).
    pub fn from_utf16(&self, u: usize) -> usize {
        let mut acc = 0;
        for (i, c) in self.chars().iter().enumerate() {
            if acc + c.len_utf16() > u {
                return i;
            }
            acc += c.len_utf16();
        }
        self.len()
    }

    pub fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.to_utf16(r.start)..self.to_utf16(r.end)
    }
    pub fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.from_utf16(r.start)..self.from_utf16(r.end)
    }
    pub fn len_utf16(&self) -> usize {
        self.to_utf16(self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_indent_toggles_with_a_width_keyed_by_the_first_id() {
        let mut d = EditDoc::from_text("ab\ncd\nef");
        d.select_range(1..4); // paragraphs 0 and 1
        d.toggle_indent(|id| 10.0 + id as f32);
        let ind: Vec<Option<f32>> = d.document().paras().iter().map(|p| p.first_indent).collect();
        assert_eq!(ind, vec![Some(10.0), Some(13.0), None]);
        assert!(d.indented());
        d.toggle_indent(|_| 99.0);
        assert!(d.document().paras().iter().all(|p| p.first_indent.is_none()));
        d.undo();
        assert_eq!(d.document().paras()[1].first_indent, Some(13.0));
    }

    #[test]
    fn tuning_selection_same_chars_undo_and_docx() {
        let mut d = EditDoc::from_text("你好你");
        d.select_range(0..1);
        d.tune(|t| t.dx_mm += 0.2);
        let moved = Tuning { dx_mm: 0.2, ..Tuning::default() };
        assert_eq!(d.tunings(), vec![Some(moved), None, None]);
        d.tune_same_chars(|t| t.scale = 1.05);
        let t = d.tunings();
        assert_eq!((t[0].map(|t| t.scale), t[1], t[2].map(|t| t.scale)), (Some(1.05), None, Some(1.05)));
        // Back to identity clears it.
        d.tune(|t| *t = Tuning::default());
        assert_eq!(d.tunings()[0], None);
        d.undo();
        assert_eq!(d.tunings()[0].map(|t| t.dx_mm), Some(0.2));
        let back = crate::docx::read(&crate::docx::write(d.document(), 1).unwrap()).unwrap();
        assert_eq!(EditDoc::from_document(back.doc).tunings(), d.tunings());
    }

    #[test]
    fn attrs_same_chars_is_one_undo_step() {
        let mut d = EditDoc::from_text("你好你");
        d.select_range(0..1);
        d.attrs_same_chars(|a| a.size_pt = Some(30.0));
        let sizes: Vec<_> = (0..3).map(|i| d.document().attrs(i).size_pt).collect();
        assert_eq!(sizes, vec![Some(30.0), None, Some(30.0)]);
        d.undo();
        assert!((0..3).all(|i| d.document().attrs(i).size_pt.is_none()));
    }

    #[test]
    fn combine_marks_all_but_the_last_and_toggles() {
        let mut d = EditDoc::from_text("你好世\n界");
        d.select_range(0..1);
        assert_eq!(d.toggle_combine(), Err("合并内容只能合并两个以及以上的字符"));
        d.select_range(2..5);
        assert_eq!(d.toggle_combine(), Err("作文格模式合并内容只能选中连续文字内容"));
        d.select_range(0..3);
        d.toggle_combine().unwrap();
        assert_eq!(d.joins(), vec![true, true, false, false, false]);
        d.toggle_combine().unwrap();
        assert_eq!(d.joins(), vec![false; 5]);
        d.undo();
        assert_eq!(d.joins(), vec![true, true, false, false, false]);
    }

    #[test]
    fn typing_selection_and_ids() {
        let mut d = EditDoc::default();
        for c in ["你", "好", "A"] {
            d.insert(c);
        }
        assert_eq!(d.text(), "你好A");
        assert_eq!(d.ids(), &[0, 1, 2]);
        d.move_to(1, false);
        d.insert("赤");
        assert_eq!(d.text(), "你赤好A");
        assert_eq!(d.ids(), &[0, 3, 1, 2], "existing chars keep their ids");
        d.select_range(1..3);
        assert_eq!(d.selected_text(), "赤好");
        d.insert("写");
        assert_eq!(d.text(), "你写A");
        assert_eq!(d.caret(), 2);
    }

    #[test]
    fn backspace_delete_and_words() {
        let mut d = EditDoc::from_text("hello 你好 world");
        d.move_to(d.len(), false);
        assert!(d.backspace(true));
        assert_eq!(d.text(), "hello 你好 ");
        d.move_to(6, false);
        assert!(d.delete(true)); // CJK char is one word
        assert_eq!(d.text(), "hello 好 ");
        d.move_to(0, false);
        assert!(!d.backspace(false), "nothing before the start");
        assert_eq!(d.word_at(2), 0..5);
    }

    #[test]
    fn undo_coalesces_typing_and_redo_restores() {
        let mut d = EditDoc::default();
        for c in "abc".chars() {
            d.insert(&c.to_string());
        }
        d.insert("\n"); // multi-char/newline insert is its own step
        assert!(d.undo());
        assert_eq!(d.text(), "abc");
        assert!(d.undo());
        assert_eq!(d.text(), "", "typing run undone in one step");
        assert!(!d.undo());
        assert!(d.redo());
        assert_eq!(d.text(), "abc");
        assert!(d.redo());
        assert_eq!(d.text(), "abc\n");
    }

    #[test]
    fn undo_restores_ids_and_redo_reuses_them() {
        let mut d = EditDoc::from_text("ab");
        d.insert("c");
        let ids = d.ids().to_vec();
        d.undo();
        d.redo();
        assert_eq!(d.ids(), &ids[..], "redo brings back the same characters, not new ones");
    }

    #[test]
    fn moving_breaks_the_typing_run() {
        let mut d = EditDoc::default();
        d.insert("a");
        d.insert("b");
        d.move_to(0, false);
        d.insert("c");
        assert!(d.undo());
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn arrows_collapse_selection() {
        let mut d = EditDoc::from_text("abcd");
        d.select_range(1..3);
        d.left(false);
        assert_eq!((d.caret(), d.has_selection()), (1, false));
        d.select_range(1..3);
        d.right(false);
        assert_eq!(d.caret(), 3);
        d.right(true);
        assert_eq!(d.selection(), 3..4);
    }

    #[test]
    fn ime_composition_then_commit() {
        let mut d = EditDoc::from_text("你");
        d.set_marked(None, "ha", Some(2..2));
        assert_eq!(d.text(), "你ha");
        assert_eq!(d.marked(), Some(1..3));
        d.set_marked(None, "hao", Some(3..3));
        assert_eq!(d.text(), "你hao");
        d.commit_text(None, "好");
        assert_eq!(d.text(), "你好");
        assert_eq!(d.marked(), None);
        assert_eq!(d.caret(), 2);
        assert!(d.undo());
        assert_eq!(d.text(), "你", "a composition is one undo step");
    }

    #[test]
    fn formatting_is_undoable_and_inherited_by_typing() {
        let mut d = EditDoc::from_text("abc");
        d.select_range(0..3);
        d.set_attrs(|a| a.colour = Some([0, 0, 255]));
        d.move_to(3, false);
        d.insert("d");
        assert_eq!(d.document().attrs(3).colour, Some([0, 0, 255]), "typing continues the run");
        d.set_align(Align::Center);
        assert_eq!(d.document().paras()[0].align, Align::Center);
        d.undo();
        d.undo();
        d.undo();
        assert_eq!(d.document().attrs(0).colour, None);
        assert_eq!(d.document().paras()[0].align, Align::Left);
    }

    #[test]
    fn utf16_round_trips() {
        let d = EditDoc::from_text("a你😀b");
        assert_eq!(d.len(), 4);
        assert_eq!(d.len_utf16(), 1 + 1 + 2 + 1);
        for i in 0..=d.len() {
            assert_eq!(d.from_utf16(d.to_utf16(i)), i);
        }
        assert_eq!(d.from_utf16(3), 2, "inside a surrogate pair rounds down");
        assert_eq!(d.range_to_utf16(&(2..3)), 2..4);
    }

    #[test]
    fn tab_walks_cells_and_adds_one_at_the_end() {
        let b = CELL_BREAK;
        let mut d = EditDoc::from_text(&format!("ab{b}cd"));
        d.move_to(1, false);
        d.next_cell();
        assert_eq!(d.caret(), 3);
        d.next_cell();
        assert_eq!(d.text(), format!("ab{b}cd{b}"));
        assert_eq!(d.caret(), 6);
        assert!(d.undo());
        assert_eq!(d.text(), format!("ab{b}cd"));
        d.move_to(4, false);
        d.prev_cell();
        assert_eq!(d.caret(), 3, "start of this cell");
        d.prev_cell();
        assert_eq!(d.caret(), 0, "then the previous one");
    }

    #[test]
    fn replace_many_is_one_undo_step_and_keeps_other_ids() {
        let mut d = EditDoc::from_text("ab-cd-ef");
        let before = d.ids().to_vec();
        d.replace_many(vec![(0..2, "X".into()), (6..8, "YZW".into()), (8..8, "!".into())]);
        assert_eq!(d.text(), "X-cd-YZW!");
        assert_eq!(&d.ids()[1..5], &before[2..6], "\"-cd-\" keeps its ids");
        assert!(d.undo());
        assert_eq!(d.text(), "ab-cd-ef");
        assert!(!d.undo());
    }

    #[test]
    fn formats_common_toggle_clear_and_paint() {
        let mut d = EditDoc::from_text("abcd");
        d.select_range(0..2);
        assert_eq!(d.common(|a| a.underline), Some(false));
        d.toggle_attr(|a| a.underline, |a, on| a.underline = on);
        d.select_range(1..3);
        // Mixed: blank in the ribbon; toggling turns it on for all.
        assert_eq!(d.common(|a| a.underline), None);
        d.toggle_attr(|a| a.underline, |a, on| a.underline = on);
        assert_eq!(d.common(|a| a.underline), Some(true));
        d.toggle_attr(|a| a.underline, |a, on| a.underline = on);
        assert_eq!(d.common(|a| a.underline), Some(false));
        assert!(d.document().attrs(0).underline);
        // 清除格式 keeps colour and tuning.
        d.select_range(0..4);
        d.set_attrs(|a| {
            a.size_pt = Some(30.0);
            a.colour = Some([1, 2, 3]);
            a.tilt = -1;
        });
        d.clear_format();
        let a = d.document().attrs(0).clone();
        assert_eq!((a.size_pt, a.tilt, a.underline, a.colour), (None, 0, false, Some([1, 2, 3])));
        // 格式刷: formats move, tuning stays.
        let src = Attrs { title: true, font: Some("楷体".into()), ..Attrs::default() };
        d.select_range(2..3);
        d.tune(|t| t.dx_mm = 1.0);
        d.paint_format(&src);
        let a = d.document().attrs(2);
        assert!(a.title && a.font.as_deref() == Some("楷体") && a.tuning.is_some());
        assert!(d.undo());
        assert!(!d.document().attrs(2).title);
        // No selection: the caret's formats.
        d.select_range(3..3);
        assert_eq!(d.common(|a| a.colour), Some(Some([1, 2, 3])));
    }

    #[test]
    fn rich_paste_keeps_each_chars_formats() {
        let mut d = EditDoc::from_text("ab");
        d.select_range(1..2);
        d.set_attrs(|a| a.underline = true);
        d.select_range(0..2);
        let (text, attrs) = (d.selected_text(), d.selected_attrs());
        d.select_range(2..2);
        d.insert_with_attrs(&text, &attrs);
        assert_eq!(d.text(), "abab");
        let u: Vec<bool> = (0..4).map(|i| d.document().attrs(i).underline).collect();
        assert_eq!(u, vec![false, true, false, true]);
        assert_eq!(d.caret(), 4);
        assert!(d.undo());
        assert_eq!(d.text(), "ab");
        // A count mismatch pastes plain text.
        d.insert_with_attrs("xyz", &attrs);
        assert_eq!(d.text(), "abxyz");
    }
}
