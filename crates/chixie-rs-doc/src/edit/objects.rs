//! Inline tables and flowcharts in the editor: insert, cell navigation (Tab), rows and columns.
//! A table is `OBJECT` cell `CELL_BREAK` cell … `TABLE_END` in the text, in paragraphs of its
//! own; its column widths live on the `OBJECT` char's [`Block`].

use std::ops::Range;

use chixie_rs_core::layout::{CELL_BREAK, table_cells};

use super::{EditDoc, LastEdit};
use crate::model::{Attrs, Block, Command, OBJECT, TABLE_END};

/// An inline table as found in the text.
#[derive(Debug, Clone, PartialEq)]
pub struct TableSpan {
    /// Index of its `OBJECT` char.
    pub anchor: usize,
    /// Index of its `TABLE_END` char.
    pub end: usize,
    /// Column widths (mm).
    pub cols: Vec<f32>,
    /// Each cell's text, row by row.
    pub cells: Vec<Range<usize>>,
}

impl TableSpan {
    pub fn ncols(&self) -> usize {
        self.cols.len().max(1)
    }
    pub fn nrows(&self) -> usize {
        self.cells.len().div_ceil(self.ncols())
    }
    /// The cell holding caret index `i` (a cell's end counts as inside it).
    pub fn cell_of(&self, i: usize) -> usize {
        self.cells.iter().position(|r| i <= r.end).unwrap_or(self.cells.len() - 1)
    }
}

/// Default width of a new table (mm) when the page gives none.
pub const TABLE_WIDTH: f32 = 120.0;

impl EditDoc {
    /// Object of each char for layout: `Some` only on `OBJECT` chars that carry one.
    pub fn blocks(&self) -> Vec<Option<&Block>> {
        let c = self.doc.chars();
        (0..self.len()).map(|i| if c[i] == OBJECT { self.doc.attrs(i).block.as_ref() } else { None }).collect()
    }

    /// The inline table whose cells hold caret index `i`.
    pub fn table_at(&self, i: usize) -> Option<TableSpan> {
        let c = self.doc.chars();
        let i = i.min(c.len());
        let anchor = c[..i].iter().rposition(|&x| x == OBJECT)?;
        let Some(Block::Table { cols }) = &self.doc.attrs(anchor).block else { return None };
        let end = (anchor + 1..c.len()).find(|&k| matches!(c[k], TABLE_END | OBJECT)).filter(|&k| c[k] == TABLE_END)?;
        if i > end {
            return None;
        }
        Some(TableSpan { anchor, end, cols: cols.clone(), cells: table_cells(c, anchor, end) })
    }

    /// The flowchart at char `i` (the caret just before or after it), with its index.
    pub fn diagram_at(&self, i: usize) -> Option<(usize, String)> {
        let c = self.doc.chars();
        [i, i.wrapping_sub(1)].into_iter().filter(|&k| k < c.len() && c[k] == OBJECT).find_map(|k| match &self.doc.attrs(k).block {
            Some(Block::Diagram { source }) => Some((k, source.clone())),
            _ => None,
        })
    }

    /// Insert an object over the selection, in paragraphs of its own: `body` (starting with the
    /// `OBJECT` char, which gets `block`). Returns the anchor index.
    fn insert_object(&mut self, body: &str, block: Block) -> usize {
        self.checkpoint(LastEdit::None);
        let sel = self.selection();
        let c = self.doc.chars();
        let before = sel.start > 0 && c[sel.start - 1] != '\n';
        let after = sel.end >= c.len() || c[sel.end] != '\n';
        let text = format!("{}{body}{}", if before { "\n" } else { "" }, if after { "\n" } else { "" });
        let base = Attrs { block: Some(block), ..self.doc.attrs_at(sel.start) };
        let new = self.doc.new_chars(&text, &base);
        let anchor = sel.start + before as usize;
        let cmd = self.doc.splice_cmd(sel, new);
        self.exec(cmd);
        self.anchor = None;
        self.marked = None;
        anchor
    }

    /// 插入表格: `rows` × `cols` empty cells, `width` mm wide; the caret goes to the first cell.
    pub fn insert_table(&mut self, rows: usize, cols: usize, width: f32) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let body: String = std::iter::once(OBJECT).chain(std::iter::repeat_n(CELL_BREAK, rows * cols - 1)).chain(std::iter::once(TABLE_END)).collect();
        let a = self.insert_object(&body, Block::Table { cols: vec![width / cols as f32; cols] });
        self.caret = a + 1;
    }

    /// 插入流程图: a flowchart from Mermaid `source`; the caret goes after it.
    pub fn insert_diagram(&mut self, source: &str) {
        let a = self.insert_object(&OBJECT.to_string(), Block::Diagram { source: source.to_string() });
        self.caret = a + 1;
    }

    /// Replace the object of the `OBJECT` char at `at` (edit a flowchart, change column widths).
    pub fn set_block(&mut self, at: usize, block: Block) {
        if self.doc.chars().get(at) != Some(&OBJECT) {
            return;
        }
        self.checkpoint(LastEdit::None);
        let a = Attrs { block: Some(block), ..self.doc.attrs(at).clone() };
        self.exec(Command::SetAttrs { at, attrs: vec![a] });
    }

    /// Tab inside an inline table: the next cell; from the last one, a new row. `false` when the
    /// caret is not in one.
    pub fn next_inline_cell(&mut self) -> bool {
        let Some(t) = self.table_at(self.caret) else { return false };
        let k = t.cell_of(self.caret);
        if k + 1 < t.cells.len() {
            self.move_to(t.cells[k + 1].start, false);
            return true;
        }
        let n = t.cells.len() % t.ncols();
        let add = if n == 0 { t.ncols() } else { t.ncols() - n };
        self.move_to(t.end, false);
        self.checkpoint(LastEdit::None);
        self.splice(t.end..t.end, &CELL_BREAK.to_string().repeat(add));
        self.caret = t.end + 1;
        true
    }

    /// Shift+Tab inside an inline table: the start of this cell, or of the one before.
    pub fn prev_inline_cell(&mut self) -> bool {
        let Some(t) = self.table_at(self.caret) else { return false };
        let k = t.cell_of(self.caret);
        let to = if self.caret > t.cells[k].start || k == 0 { t.cells[k].start } else { t.cells[k - 1].start };
        self.move_to(to, false);
        true
    }

    /// Whether deleting char `k` would tear an inline table apart (its markers and cell breaks
    /// only go with the whole table or a row / column command).
    pub(super) fn guards_table(&self, k: usize) -> bool {
        let c = self.doc.chars();
        match c.get(k) {
            Some(&TABLE_END) => true,
            Some(&OBJECT) => matches!(self.doc.attrs(k).block, Some(Block::Table { .. })),
            Some(&CELL_BREAK) => self.table_at(k).is_some(),
            _ => false,
        }
    }

    /// Several splices and an optional new block, into the current undo step (the caller opens
    /// it). Splices must not overlap; they are applied last-first.
    fn table_edit(&mut self, mut edits: Vec<(Range<usize>, String)>, block: Option<(usize, Block)>, caret: usize) {
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        for (r, text) in edits {
            self.splice(r, &text);
        }
        if let Some((at, b)) = block {
            let a = Attrs { block: Some(b), ..self.doc.attrs(at).clone() };
            self.exec(Command::SetAttrs { at, attrs: vec![a] });
        }
        self.caret = caret.min(self.len());
        self.anchor = None;
    }

    /// The table at the caret with its last row filled up, so every row has every column.
    fn full_table(&mut self) -> Option<TableSpan> {
        let t = self.table_at(self.caret)?;
        let n = t.cells.len() % t.ncols();
        if n == 0 {
            return Some(t);
        }
        let caret = self.caret;
        self.splice(t.end..t.end, &CELL_BREAK.to_string().repeat(t.ncols() - n));
        self.caret = caret;
        self.table_at(caret)
    }

    /// 在下方插入行.
    pub fn add_row(&mut self) -> bool {
        let Some(t) = self.table_at(self.caret) else { return false };
        let r = t.cell_of(self.caret) / t.ncols();
        let last = ((r + 1) * t.ncols()).min(t.cells.len()) - 1;
        let at = t.cells[last].end;
        self.checkpoint(LastEdit::None);
        self.table_edit(vec![(at..at, CELL_BREAK.to_string().repeat(t.ncols()))], None, at + 1);
        true
    }

    /// 删除行 (the last row takes the whole table with it).
    pub fn delete_row(&mut self) -> bool {
        let Some(t) = self.table_at(self.caret) else { return false };
        if t.nrows() <= 1 {
            return self.delete_table();
        }
        let r = t.cell_of(self.caret) / t.ncols();
        let first = r * t.ncols();
        let next = first + t.ncols();
        let range = if next < t.cells.len() { t.cells[first].start..t.cells[next].start } else { t.cells[first].start - 1..t.cells[t.cells.len() - 1].end };
        let caret = range.start.min(t.cells[first].start);
        self.checkpoint(LastEdit::None);
        self.table_edit(vec![(range, String::new())], None, caret);
        true
    }

    /// 在右侧插入列: column `c + 1`, taking half of column `c`'s width.
    pub fn add_col(&mut self) -> bool {
        if self.table_at(self.caret).is_none() {
            return false;
        }
        self.checkpoint(LastEdit::None);
        let Some(t) = self.full_table() else { return false };
        let n = t.ncols();
        let c = t.cell_of(self.caret) % n;
        let edits = (0..t.nrows()).map(|r| (t.cells[r * n + c].end..t.cells[r * n + c].end, CELL_BREAK.to_string())).collect();
        let mut cols = t.cols.clone();
        if cols.is_empty() {
            cols.push(TABLE_WIDTH);
        }
        let half = cols[c] / 2.0;
        cols[c] = half;
        cols.insert(c + 1, half);
        // Each earlier row gains one break before the caret's cell; the new cell follows it.
        let k = t.cell_of(self.caret);
        let caret = t.cells[k].end + k / n + 1;
        self.table_edit(edits, Some((t.anchor, Block::Table { cols })), caret);
        true
    }

    /// 删除列 (the last column takes the whole table with it).
    pub fn delete_col(&mut self) -> bool {
        let Some(t0) = self.table_at(self.caret) else { return false };
        if t0.ncols() <= 1 {
            return self.delete_table();
        }
        self.checkpoint(LastEdit::None);
        let Some(t) = self.full_table() else { return false };
        let n = t.ncols();
        let c = t.cell_of(self.caret) % n;
        let edits = (0..t.nrows())
            .map(|r| {
                let cell = &t.cells[r * n + c];
                // The cell and the break after it; the last column takes the break before.
                let range = if c + 1 < n { cell.start..cell.end + 1 } else { cell.start - 1..cell.end };
                (range, String::new())
            })
            .collect();
        let mut cols = t.cols.clone();
        cols.remove(c.min(cols.len() - 1));
        self.table_edit(edits, Some((t.anchor, Block::Table { cols })), t.anchor + 1);
        true
    }

    /// 删除表格: the table and the paragraph it stood in.
    pub fn delete_table(&mut self) -> bool {
        let Some(t) = self.table_at(self.caret) else { return false };
        let c = self.doc.chars();
        // Take one of the newlines around it, so no empty paragraph is left behind.
        let range = if t.end + 1 < c.len() && c[t.end + 1] == '\n' {
            t.anchor..t.end + 2
        } else if t.anchor > 0 && c[t.anchor - 1] == '\n' {
            t.anchor - 1..t.end + 1
        } else {
            t.anchor..t.end + 1
        };
        let caret = range.start;
        self.checkpoint(LastEdit::None);
        self.table_edit(vec![(range, String::new())], None, caret);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text with markers spelled out: `[` anchor, `|` cell break, `]` end.
    fn shown(d: &EditDoc) -> String {
        d.text().chars().map(|c| match c {
            OBJECT => '[',
            CELL_BREAK => '|',
            TABLE_END => ']',
            c => c,
        }).collect()
    }

    fn table_doc() -> EditDoc {
        let mut d = EditDoc::from_text("ab");
        d.move_to(1, false);
        d.insert_table(2, 2, 60.0);
        d
    }

    #[test]
    fn insert_table_gets_its_own_paragraph_and_caret_in_cell_one() {
        let d = table_doc();
        assert_eq!(shown(&d), "a\n[|||]\nb");
        assert_eq!(d.caret(), 3);
        let t = d.table_at(d.caret()).unwrap();
        assert_eq!((t.anchor, t.end, t.cols.clone(), t.cells.len()), (2, 6, vec![30.0, 30.0], 4));
        assert!(d.blocks()[2].is_some() && d.blocks().iter().filter(|b| b.is_some()).count() == 1);
        assert!(d.document().is_consistent());
    }

    #[test]
    fn typing_in_a_cell_is_plain_text() {
        let mut d = table_doc();
        d.insert("x");
        assert_eq!(shown(&d), "a\n[x|||]\nb");
        assert!(d.document().attrs(3).block.is_none(), "typed text never carries the table");
    }

    #[test]
    fn tab_walks_the_cells_then_adds_a_row_and_undo_restores() {
        let mut d = table_doc();
        let before = d.document().clone();
        for want in [4, 5, 6] {
            assert!(d.next_inline_cell());
            assert_eq!(d.caret(), want);
        }
        assert!(d.next_inline_cell());
        assert_eq!(shown(&d), "a\n[|||||]\nb");
        assert_eq!(d.caret(), 7);
        assert!(d.prev_inline_cell());
        assert_eq!(d.caret(), 6);
        d.undo();
        assert!(d.document().same_content(&before));
        // Outside a table Tab is not ours.
        d.move_to(0, false);
        assert!(!d.next_inline_cell());
    }

    #[test]
    fn backspace_and_delete_stop_at_cell_edges() {
        let mut d = table_doc();
        d.insert("x");
        assert!(!d.delete(false), "the cell break after x stays");
        assert!(d.backspace(false));
        assert!(!d.backspace(false), "the anchor before the first cell stays");
        d.move_to(7, false); // after the end marker, before '\n'
        assert!(!d.backspace(false));
        assert_eq!(shown(&d), "a\n[|||]\nb");
    }

    #[test]
    fn rows_and_columns() {
        let mut d = table_doc();
        d.insert("1");
        d.next_inline_cell();
        d.insert("2");
        d.next_inline_cell();
        d.insert("3");
        assert_eq!(shown(&d), "a\n[1|2|3|]\nb");
        d.move_to(4, false); // in cell "1"
        assert!(d.add_row());
        assert_eq!(shown(&d), "a\n[1|2|||3|]\nb");
        assert!(d.delete_row());
        assert_eq!(shown(&d), "a\n[1|2|3|]\nb");
        d.move_to(4, false);
        assert!(d.add_col());
        assert_eq!(shown(&d), "a\n[1||2|3||]\nb");
        assert_eq!(d.caret(), 5, "caret in the new cell");
        assert_eq!(d.table_at(5).unwrap().cols, vec![15.0, 15.0, 30.0]);
        d.move_to(6, false); // in cell "2", the last column
        assert!(d.delete_col());
        assert_eq!(shown(&d), "a\n[1||3|]\nb");
        assert_eq!(d.table_at(4).unwrap().cols, vec![15.0, 15.0]);
        // One undo per command.
        d.undo();
        assert_eq!(shown(&d), "a\n[1||2|3||]\nb");
        d.undo();
        assert_eq!(shown(&d), "a\n[1|2|3|]\nb");
    }

    #[test]
    fn delete_table_takes_its_paragraph() {
        let mut d = table_doc();
        assert!(d.delete_table());
        assert_eq!(d.text(), "a\nb");
        assert!(d.document().is_consistent());
    }

    #[test]
    fn diagrams_insert_and_edit() {
        let mut d = EditDoc::from_text("");
        d.insert_diagram("graph TD\nA-->B");
        assert_eq!(shown(&d), "[\n");
        assert_eq!(d.diagram_at(d.caret()).map(|x| x.0), Some(0));
        d.set_block(0, Block::Diagram { source: "graph LR\nA-->C".into() });
        assert_eq!(d.diagram_at(0).unwrap().1, "graph LR\nA-->C");
        d.undo();
        assert_eq!(d.diagram_at(0).unwrap().1, "graph TD\nA-->B");
        d.move_to(1, false);
        assert!(d.backspace(false), "a flowchart deletes like one char");
        assert_eq!(d.text(), "\n");
    }
}
