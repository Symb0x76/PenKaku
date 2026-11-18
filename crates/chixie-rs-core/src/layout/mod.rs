//! Text → positioned pen strokes in machine mm (`Document`).
//!
//! The preview and the G-code emitter (`chixie-rs-output`) both consume the same `Document`, so what is
//! shown is what is drawn. Glyph units are scaled to mm and Y is flipped (glyph y-down → machine
//! y-up). The page origin is the top-left corner, in machine mm.
//!
//! With every effect off, the arithmetic here keeps the exact float expressions and operation
//! order of the original `writing::text_to_gcode`, so its output stays byte-identical. The golden
//! test and the `legacy_oracle` test (both in `chixie-rs-output`) check this; don't reorder the math.

use crate::deformation::Deformation;
use crate::font::Glyph;
use crate::font::fontset::Glyphs;
#[cfg(test)]
use crate::font::Font;
use crate::handwriting::{HandwritingStyle, Jitter, SALT_DEFORM, is_cjk, rng_for};

pub mod kinsoku;
mod pen;
mod segment;
#[cfg(test)]
#[cfg(test)]
mod tests;

pub use pen::glyph_for;
use pen::{Pen, draw_char};

/// One pen-down polyline in machine mm.
pub type Stroke = Vec<(f32, f32)>;

/// Paragraph alignment. Justify stretches wrapped lines only; in a grid, centre and right move
/// by whole cells and justify does nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// 字符微调: a per-character nudge, size and shape change. Identity = `Tuning::default()`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Tuning {
    /// Move right (mm).
    pub dx_mm: f32,
    /// Move down (mm); 上移 is negative.
    pub dy_mm: f32,
    /// Size factor (A+ / A−), about the glyph's bottom-left.
    pub scale: f32,
    /// Width factor (变胖 / 变瘦).
    pub width: f32,
    /// 变形: each bump draws a new deformation for this character.
    pub reroll: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { dx_mm: 0.0, dy_mm: 0.0, scale: 1.0, width: 1.0, reroll: 0 }
    }
}

impl Tuning {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// Paragraph formatting (one per `'\n'`-separated paragraph).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Para {
    pub align: Align,
    /// First-line indent, mm (two cells in a grid); `None` = the handwriting default.
    pub first_indent: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub strokes: Vec<Stroke>,
    /// Template ruling (row lines, cell boxes, column dividers) in machine mm. Shown in the
    /// editor only; never written: templates describe pre-printed paper.
    pub guides: Vec<Guide>,
    /// Character index (into the laid-out text) of each stroke; same length as `strokes` when
    /// produced by `layout_ids`, empty for hand-built pages. The resume map (断点续写) uses it.
    pub stroke_chars: Vec<usize>,
}

impl Page {
    /// First stroke (in writing order) of the first character at or after `i` in text order that
    /// has strokes; `strokes.len()` if none. Strokes may be reordered (弓字型), so this picks the
    /// character first, then its earliest stroke.
    pub fn first_stroke_from(&self, i: usize) -> usize {
        let Some(c) = self.stroke_chars.iter().copied().filter(|&c| c >= i).min() else { return self.strokes.len() };
        self.stroke_chars.iter().position(|&x| x == c).unwrap_or(self.strokes.len())
    }
}

/// One ruling segment (machine mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub dashed: bool,
}

/// Nominal glyph height in font units (CJK glyphs span about 0..7). Used to sit text on ruled
/// rows and centre it in grid cells; both placements match ChiXie3 captures (docs/protocol.md).
pub const EM_UNITS: f32 = 7.0;

/// How lines are placed on a page.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Ruling {
    /// Lines every `line_height` from the top (no guides).
    #[default]
    Free,
    /// Ruled paper: `rows` lines per page (a random count in the range, drawn per page),
    /// `columns` equal columns `column_gap` apart, filled top to bottom then left to right.
    /// `widths`: per-column widths (mm) when they don't share the width equally (empty = equal).
    /// `band`: dual-line mode, an upper band (mm) above the writing line; text goes below it, or
    /// into it with `upper`.
    Rows { rows: (u32, u32), columns: u32, column_gap: f32, widths: Vec<f32>, band: Option<f32>, upper: bool, dashed: bool, divider: bool },
    /// Composition grid: one character per `cell` (w, h), `gap` (col, row) between cells.
    Grid { rows: u32, cols: u32, cell: (f32, f32), gap: (f32, f32) },
    /// Table form: `pages[k]` lists page k's cells in writing order; the template's pages repeat.
    /// The text's [`CELL_BREAK`]-separated segments fill the cells one by one.
    Table { pages: Vec<Vec<CellBox>> },
}

/// Ends a table cell's text: the next character starts the next cell (Tab in the editor).
pub const CELL_BREAK: char = '\u{1F}';

/// Vertical placement of a cell's lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// What a cell does with text that needs more rows than it has (ChiXie3 内容溢出 方案1/2/3).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Overflow {
    /// 方案1 缩放字符: shrink the characters until the text fits, down to `min` × their size.
    Shrink { min: f32 },
    /// 方案2 增加行数: up to `max` rows; the characters shrink with the rows.
    AddRows { max: u32 },
    /// 方案3 忽略溢出内容: what does not fit is not written.
    Clip,
}

impl Default for Overflow {
    fn default() -> Self {
        Overflow::Shrink { min: 0.4 }
    }
}

/// One table cell as layout sees it: a box (mm from the writable top-left, y down) holding
/// `rows` writing rows inside its padding, with its own size, alignment and overflow policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rows: u32,
    /// (left, top, right, bottom) mm.
    pub padding: (f32, f32, f32, f32),
    pub align: Align,
    pub valign: VAlign,
    /// Character size relative to the document's.
    pub scale: f32,
    pub overflow: Overflow,
}

impl Default for CellBox {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 10.0, h: 10.0, rows: 1, padding: (1.0, 0.0, 1.0, 0.0), align: Align::Left, valign: VAlign::Top, scale: 1.0, overflow: Overflow::default() }
    }
}

/// How a cell's text was finally set: size relative to the document's, and rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellFit {
    pub scale: f32,
    pub rows: u32,
}

/// A filled table cell after layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellInfo {
    pub page: usize,
    /// Index into the template page's cells.
    pub cell: usize,
    /// Which `CELL_BREAK`-separated segment of the text this is.
    pub seg: usize,
    /// Its chars: `start..end` (`end` is the break, or the text end).
    pub start: usize,
    pub end: usize,
    /// (left, top, right, bottom) in machine mm.
    pub rect: (f32, f32, f32, f32),
    pub fit: CellFit,
    /// First char that did not fit (not written); `None` when everything fits.
    pub overflow_from: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub pages: Vec<Page>,
    /// Characters with no glyph in the font (each skipped with a space-width advance), deduped,
    /// in first-seen order.
    pub missing: Vec<char>,
    /// Index of every character drawn as a 缺字 placeholder (no font has it), in text order.
    pub missing_at: Vec<usize>,
    /// One box per input character (glyphs, spaces, newlines, missing glyphs), in text order.
    pub boxes: Vec<CharBox>,
    /// Every line, in order; caret positions on a line are `start..=end` (char indices).
    pub lines: Vec<LineInfo>,
    /// Layout origin (machine mm): caret x on an empty line.
    pub origin: (f32, f32),
    /// Lines that still start with a closing mark or end with an opening mark after a wrap
    /// (避头尾 off, or nothing could be moved): the 位置合理性检测 badge.
    pub punct_issues: Vec<usize>,
    /// Laid out on a composition grid (one character per cell).
    pub grid: bool,
    /// Table forms: every filled cell, in writing order.
    pub cells: Vec<CellInfo>,
}

impl Document {
    /// Cells whose text does not fit (the 内容溢出 badge).
    pub fn overflowing(&self) -> impl Iterator<Item = &CellInfo> {
        self.cells.iter().filter(|c| c.overflow_from.is_some())
    }
}

/// Where one input character sits (machine mm; y up, so `top > bottom`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharBox {
    pub page: usize,
    pub line: usize,
    pub x0: f32,
    pub x1: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineInfo {
    pub page: usize,
    pub top: f32,
    pub bottom: f32,
    /// First char index on the line.
    pub start: usize,
    /// Caret index at the line's end: the index of its '\n', or of the first char of the next
    /// (wrapped) line, or the text length on the last line.
    pub end: usize,
}

/// A caret position on the page (machine mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub page: usize,
    pub line: usize,
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Document {
    /// Line holding caret index `i`. At a wrap boundary the caret belongs to the next line.
    pub fn line_of(&self, i: usize) -> usize {
        self.lines
            .iter()
            .rposition(|l| l.start <= i && i <= l.end)
            .unwrap_or(self.lines.len().saturating_sub(1))
    }

    /// Caret geometry for index `i` (0..=text length).
    pub fn caret(&self, i: usize) -> Caret {
        let li = self.line_of(i);
        let Some(l) = self.lines.get(li) else {
            return Caret { page: 0, line: 0, x: self.origin.0, top: self.origin.1, bottom: self.origin.1 - 10.0 };
        };
        let x = match self.boxes.get(i) {
            Some(b) if b.line == li => b.x0,
            _ if i > l.start => self.boxes[i - 1].x1,
            _ => self.origin.0,
        };
        Caret { page: l.page, line: li, x, top: l.top, bottom: l.bottom }
    }

    /// Nearest caret index on line `li` for x (mm).
    pub fn index_on_line(&self, li: usize, x: f32) -> usize {
        let l = self.lines[li];
        self.boxes[l.start..l.end].iter().position(|b| x < (b.x0 + b.x1) / 2.0).map_or(l.end, |k| l.start + k)
    }

    /// Caret index for a point on `page` (mm): the line under y (or the nearest), then nearest x.
    pub fn index_at(&self, page: usize, x: f32, y: f32) -> usize {
        let best = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.page == page)
            .min_by(|(_, a), (_, b)| {
                let d = |l: &LineInfo| if y <= l.top && y >= l.bottom { 0.0 } else { (y - (l.top + l.bottom) / 2.0).abs() };
                d(a).total_cmp(&d(b))
            })
            .map(|(i, _)| i);
        match best {
            Some(li) => self.index_on_line(li, x),
            None => self.boxes.len(),
        }
    }

    /// Caret index one line up (`dir < 0`) or down, keeping x; clamps to start / end of text.
    pub fn vertical_move(&self, i: usize, dir: i32) -> usize {
        let li = self.line_of(i) as i64 + dir as i64;
        if li < 0 {
            return 0;
        }
        if li as usize >= self.lines.len() {
            return self.boxes.len();
        }
        self.index_on_line(li as usize, self.caret(i).x)
    }

    /// (min_x, min_y, max_x, max_y) over every point on every page, in machine mm; `None` when
    /// nothing is drawn. Pages share one coordinate frame (each sheet is drawn from the origin).
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut it = self.pages.iter().flat_map(|p| p.strokes.iter().flatten());
        let &(x, y) = it.next()?;
        Some(it.fold((x, y, x, y), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y))))
    }

    pub fn stroke_count(&self) -> usize {
        self.pages.iter().map(|p| p.strokes.len()).sum()
    }
}

/// Writable area measured from `origin`: x grows right, y grows *down* the page (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBox {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct PageParams {
    /// mm per glyph unit (character size). ChiXie3 derives it from font size × PXScale; the
    /// calibrated value comes from M1 (see docs/protocol.md). TODO: verify against capture A.
    pub mm_per_unit: f32,
    /// Extra mm added after each glyph's advance (character spacing).
    pub char_spacing: f32,
    /// Advance in mm for ' ' when the font has no space glyph, and for missing characters.
    pub space_width: f32,
    /// Baseline-to-baseline distance in mm for '\n' and wrapping.
    pub line_height: f32,
    /// Machine mm where the first glyph's (0,0) lands.
    pub origin: (f32, f32),
    /// `None`: one unbounded page (CLI default). `Some`: wrap at `width`, new page past `height`.
    pub page: Option<PageBox>,
    /// Handwriting jitter. When enabled it replaces `char_spacing` and `space_width`.
    pub style: HandwritingStyle,
    pub deformation: Deformation,
    /// Line placement; anything but `Free` needs `page`.
    pub ruling: Ruling,
    /// 避头尾: never start a wrapped line with a closing mark or end one with an opening mark
    /// (ruled/free: pull chars down; grid: hang closing marks in the last cell).
    pub kinsoku: bool,
    /// Grid: two Latin letters share a cell like two digits do.
    pub grid_pair_letters: bool,
    /// Grid: a word (jieba, feature `segment`) that would cross a row end starts the next row.
    pub grid_keep_words: bool,
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            mm_per_unit: 1.0,
            char_spacing: 0.0,
            space_width: 3.5,
            line_height: 10.0,
            origin: (0.0, 0.0),
            page: None,
            style: HandwritingStyle::default(),
            deformation: Deformation::default(),
            ruling: Ruling::Free,
            kinsoku: false,
            grid_pair_letters: false,
            grid_keep_words: false,
        }
    }
}

/// Lay out `text` with `font`, using character positions as ids (`0..n`). Unknown characters
/// advance like a space and are listed in `Document::missing`. See [`layout_ids`].
pub fn layout(text: &str, font: &(impl Glyphs + ?Sized), p: &PageParams) -> Document {
    let chars: Vec<char> = text.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    layout_ids(&chars, &ids, font, p)
}

/// Lay out `chars` with stable per-character `ids` (same length). Each character's jitter and
/// deformation come from its own id-keyed random stream, so editing one character never moves the
/// others; a line's start offset is keyed by the id of its first character.
///
/// With jitter and deformation off, every coordinate uses the original expressions (see module
/// docs), so the output stays byte-identical to the pre-refactor pipeline.
pub fn layout_ids(chars: &[char], ids: &[u64], font: &(impl Glyphs + ?Sized), p: &PageParams) -> Document {
    layout_doc(chars, ids, &[], font, p)
}

/// [`layout_ids`] with paragraph formatting: `paras[k]` applies to the k-th `'\n'`-separated
/// paragraph (missing entries are left-aligned with the default indent).
pub fn layout_doc(chars: &[char], ids: &[u64], paras: &[Para], font: &(impl Glyphs + ?Sized), p: &PageParams) -> Document {
    layout_flow(&Flow { chars, ids, paras, joins: &[], tuning: &[] }, font, p)
}

/// Everything layout reads from a document.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flow<'a> {
    pub chars: &'a [char],
    /// One stable id per char.
    pub ids: &'a [u64],
    /// One per paragraph; missing entries use the default.
    pub paras: &'a [Para],
    /// 合并内容: `joins[k]` puts char `k + 1` in the same grid cell as char `k`. Missing = false.
    pub joins: &'a [bool],
    /// 字符微调 per char; missing or `None` = untouched.
    pub tuning: &'a [Option<Tuning>],
}

/// Half-width in a grid: two in a row share one cell. Digits and the half space always;
/// Latin letters when `letters` (参数设置 › 数字/字母两个一格).
pub fn half_width(c: char, letters: bool) -> bool {
    c.is_ascii_digit() || c == HALF_SPACE || (letters && c.is_ascii_alphabetic())
}

/// 半格空格 (Shift+Space): half as wide as a space.
pub const HALF_SPACE: char = '\u{2009}';

/// Lay out a [`Flow`]. See [`layout_ids`].
///
/// Table forms: cells whose text overflows are fitted by their policy (shrink, more rows) with a
/// few extra passes; every segment always lands in the same cell, so the cells are searched
/// together and independently.
pub fn layout_flow(f: &Flow, font: &(impl Glyphs + ?Sized), p: &PageParams) -> Document {
    let Ruling::Table { pages } = &p.ruling else { return layout_pass(f, font, p, &[]) };
    let pages: Vec<Vec<CellBox>> = pages.iter().filter(|c| !c.is_empty()).cloned().collect();
    if pages.is_empty() {
        return layout_pass(f, font, &PageParams { ruling: Ruling::Free, ..p.clone() }, &[]);
    }
    let q = PageParams { ruling: Ruling::Table { pages: pages.clone() }, ..p.clone() };
    let doc = layout_pass(f, font, &q, &[]);
    let boxed = |c: &CellInfo| pages[c.page % pages.len()][c.cell];
    let todo: Vec<(usize, CellBox)> = doc.overflowing().map(|c| (c.seg, boxed(c))).filter(|(_, b)| b.overflow != Overflow::Clip).collect();
    if todo.is_empty() {
        return doc;
    }
    let segs = doc.cells.iter().map(|c| c.seg + 1).max().unwrap_or(0);
    let mut fits: Vec<Option<CellFit>> = vec![None; segs];
    let fits_now = |d: &Document, seg: usize| d.cells.iter().any(|c| c.seg == seg && c.overflow_from.is_none());
    // 方案1: binary search for the largest size that fits (≥ min).
    let shrink: Vec<(usize, f32, u32)> = todo.iter().filter_map(|&(seg, b)| match b.overflow {
        Overflow::Shrink { min } => Some((seg, min.clamp(0.05, 1.0), b.rows.max(1))),
        _ => None,
    }).collect();
    let mut range: Vec<(f32, f32)> = shrink.iter().map(|&(_, min, _)| (min, 1.0)).collect();
    if !shrink.is_empty() {
        for _ in 0..8 {
            for (k, &(seg, _, rows)) in shrink.iter().enumerate() {
                fits[seg] = Some(CellFit { scale: (range[k].0 + range[k].1) / 2.0, rows });
            }
            let d = layout_pass(f, font, &q, &fits);
            for (k, &(seg, ..)) in shrink.iter().enumerate() {
                let mid = (range[k].0 + range[k].1) / 2.0;
                if fits_now(&d, seg) { range[k].0 = mid } else { range[k].1 = mid }
            }
        }
        for (k, &(seg, _, rows)) in shrink.iter().enumerate() {
            fits[seg] = Some(CellFit { scale: range[k].0, rows });
        }
    }
    // 方案2: the fewest extra rows that fit; characters shrink in step with the rows.
    let mut grow: Vec<(usize, u32, u32)> = todo.iter().filter_map(|&(seg, b)| match b.overflow {
        Overflow::AddRows { max } => Some((seg, b.rows.max(1), max.max(b.rows.max(1)))),
        _ => None,
    }).collect();
    let mut extra = 1;
    while !grow.is_empty() {
        for &(seg, rows, max) in &grow {
            let r = (rows + extra).min(max);
            fits[seg] = Some(CellFit { scale: rows as f32 / r as f32, rows: r });
        }
        let d = layout_pass(f, font, &q, &fits);
        grow.retain(|&(seg, rows, max)| !fits_now(&d, seg) && rows + extra < max);
        extra += 1;
    }
    layout_pass(f, font, &q, &fits)
}

/// One layout pass; `fits[seg]` overrides a table cell's size and rows.
fn layout_pass(f: &Flow, font: &(impl Glyphs + ?Sized), p: &PageParams, fits: &[Option<CellFit>]) -> Document {
    let Flow { chars, ids, paras, joins, tuning } = *f;
    assert_eq!(chars.len(), ids.len(), "one id per character");
    let para = |k: usize| paras.get(k).copied().unwrap_or_default();
    let join = |k: usize| joins.get(k).copied().unwrap_or(false);
    let tune = |k: usize| tuning.get(k).copied().flatten().unwrap_or_default();
    let mut para_ix = 0;
    let mut s = p.mm_per_unit;
    let table = matches!(p.ruling, Ruling::Table { .. });
    let (ox, oy) = p.origin;
    let jitter_on = p.style.enabled;
    let deform_on = p.deformation.is_active();
    let mut jit = Jitter::new(&p.style);

    let grid = match p.ruling {
        Ruling::Grid { cols, cell, gap, .. } if p.page.is_some() => Some((cols.max(1), cell.0, gap.0)),
        _ => None,
    };
    let mut pen = Pen {
        p,
        chars,
        align: para(0).align,
        line_stroke: 0,
        pitch: grid.map(|(_, cw, cg)| cw + cg),
        doc: Document { origin: p.origin, grid: grid.is_some(), ..Default::default() },
        page: Page::default(),
        page_ix: 0,
        cx: 0.0,
        line_top: 0.0,
        line_start: 0,
        slot: 0,
        col: 0,
        rows: 0,
        line_x: 0.0,
        line_w: f32::INFINITY,
        row_top: 0.0,
        row_h: p.line_height,
        text_top: 0.0,
        em: EM_UNITS * s,
        scale: s,
        fits,
        cell: 0,
        seg: 0,
        cell_first_line: 0,
        cell_first_stroke: 0,
        cell_first_char: 0,
        overflow_from: None,
    };
    pen.rows = pen.rows_for(0);
    pen.enter_slot();
    pen.page_guides();
    if table {
        pen.start_cell(0);
        s = pen.scale;
    }
    let mut prev: Option<char> = None;
    // Jitter: the next placed character starts a line (and maybe a paragraph).
    // Table cells never take a first-line indent.
    let (mut fresh_line, mut fresh_paragraph) = (true, !table);
    // Grid: closing marks hung past the row's last cell so far.
    let mut hung = 0u32;
    // Grid with 作文格分词: where each word starting at a char ends.
    let words = if grid.is_some() && p.grid_keep_words { segment::word_ends(chars) } else { Vec::new() };
    // Grid: the open shared cell (x, next slot, slots, squeezed 合并内容 run vs half-width pair).
    let mut shared: Option<(f32, usize, usize, bool)> = None;
    // Scales for a char in a shared cell's slot `w` mm wide: a 合并内容 run is squeezed across;
    // a half-width char keeps its size unless it would not fit.
    let slot_scale = |s: f32, g: Option<&Glyph>, squeeze: bool, n: usize, w: f32| {
        if squeeze {
            return (s / n as f32, s);
        }
        let fit = g.map_or(s, |g| if g.advance > 0.0 { s.min(0.9 * w / g.advance) } else { s });
        (fit, fit)
    };

    // A `while` loop: 避头尾 can step back and lay a few chars again on the next line. Every
    // char's randomness is keyed by its id, so laying it again gives the same strokes.
    let mut next = 0;
    while next < chars.len() {
        let i = next;
        next += 1;
        let (ch, id) = (chars[i], ids[i]);
        // Strokes so far belong to the previous character (before any page break below).
        if i > 0 {
            pen.tag_strokes(i - 1);
        }
        if fresh_line && ch != '\n' {
            if jitter_on {
                pen.cx = jit.line_start(id);
            }
            let indent = if fresh_paragraph { para(para_ix).first_indent.or_else(|| if jitter_on { jit.indent(id) } else { None }) } else { None };
            match (indent, grid) {
                (Some(_), Some((cols, ..))) => pen.col = 2.min(cols - 1),
                (Some(w), None) => pen.cx += w,
                (None, _) => {}
            }
            fresh_line = false;
            fresh_paragraph = false;
        }
        let x0 = pen.cx;
        if let (Some((cols, cw, cg)), false) = (grid, ch == '\n' || ch == CELL_BREAK) {
            // One character per cell, centred; spaces and missing glyphs take a cell too.
            // 合并内容 groups and pairs of half-width chars share one cell, one slot each.
            if let Some((cell_x, slot, n, squeeze)) = shared {
                let w = cw / n as f32;
                let x = cell_x + w * slot as f32;
                let (sx, sy) = slot_scale(s, font.glyph(ch, id), squeeze, n, w);
                let adv = font.glyph(ch, id).map_or(0.0, |g| g.advance * sx);
                draw_char(&mut pen, font, &mut jit, ch, id, x + (w - adv) / 2.0, sx, sy, 0.0, tune(i));
                pen.cx = x + w;
                pen.push_box(x);
                shared = (slot + 1 < n).then_some((cell_x, slot + 1, n, squeeze));
                prev = Some(ch);
                continue;
            }
            if pen.col >= cols {
                if p.kinsoku && kinsoku::is_closing(ch) && hung < 2 && i > pen.line_start {
                    // 避头尾 in a grid: hang the mark in the lower right of the row's last cell
                    // (half size; a second one beside it), as composition paper is written.
                    let x = (cols - 1) as f32 * (cw + cg) + cw * (0.5 + 0.25 * hung as f32);
                    let lower = pen.em * 0.5;
                    draw_char(&mut pen, font, &mut jit, ch, id, x, s * 0.5, s * 0.5, lower, tune(i));
                    pen.cx = x + cw * 0.25;
                    pen.push_box(x);
                    hung += 1;
                    prev = Some(ch);
                    continue;
                }
                pen.newline(i, i);
            } else if p.kinsoku && pen.col + 1 == cols && pen.col > 0 && kinsoku::is_opening(ch) && chars.get(i + 1).is_some_and(|&c| c != '\n') {
                // An opening mark never takes a row's last cell; it starts the next row.
                pen.newline(i, i);
            }
            // 作文格分词: a word that fits a row but not the rest of this one starts the next.
            if let Some(&Some(end)) = words.get(i)
                && pen.col > 0
                && ((end - i) as u32) <= cols
                && pen.col + (end - i) as u32 > cols
            {
                pen.newline(i, i);
            }
            hung = 0;
            let cell_x = pen.col as f32 * (cw + cg);
            // Chars sharing this cell: a 合并内容 run, or two half-width chars.
            let n = if join(i) {
                (i..chars.len()).take_while(|&k| join(k) && chars.get(k + 1).is_some_and(|&c| c != '\n')).count() + 1
            } else if half_width(ch, p.grid_pair_letters) && chars.get(i + 1).is_some_and(|&c| half_width(c, p.grid_pair_letters)) {
                2
            } else {
                1
            };
            if n > 1 {
                let w = cw / n as f32;
                let (sx, sy) = slot_scale(s, font.glyph(ch, id), join(i), n, w);
                let adv = font.glyph(ch, id).map_or(0.0, |g| g.advance * sx);
                draw_char(&mut pen, font, &mut jit, ch, id, cell_x + (w - adv) / 2.0, sx, sy, 0.0, tune(i));
                pen.cx = cell_x + w;
                pen.col += 1;
                pen.push_box(cell_x);
                shared = Some((cell_x, 1, n, join(i)));
                prev = Some(ch);
                continue;
            }
            if ch != ' ' {
                let adv = font.glyph(ch, id).map_or(0.0, |g| g.advance * s);
                draw_char(&mut pen, font, &mut jit, ch, id, cell_x + (cw - adv) / 2.0, s, s, 0.0, tune(i));
            }
            pen.cx = cell_x + cw;
            pen.col += 1;
            pen.push_box(cell_x);
            prev = Some(ch);
            continue;
        }
        match ch {
            '\n' => {
                pen.push_box(x0);
                pen.newline(i, i + 1);
                para_ix += 1;
                if !table {
                    pen.align = para(para_ix).align;
                }
                (fresh_line, fresh_paragraph) = (true, !table);
                prev = None;
                continue;
            }
            CELL_BREAK => {
                pen.push_box(x0);
                if table {
                    pen.next_cell(i, i + 1);
                    s = pen.scale;
                } else {
                    // Outside a table form a cell break reads as a line break.
                    pen.newline(i, i + 1);
                }
                fresh_line = true;
                prev = None;
                continue;
            }
            ' ' => {
                if jitter_on {
                    pen.cx += jit.space(prev, id);
                } else {
                    pen.cx += font.space().map(|gl| gl.advance * s).unwrap_or(p.space_width);
                    pen.cx += p.char_spacing;
                }
                pen.push_box(x0);
                continue;
            }
            HALF_SPACE => {
                let full = if jitter_on { jit.space(prev, id) } else { font.space().map(|gl| gl.advance * s).unwrap_or(p.space_width) + p.char_spacing };
                pen.cx += full / 2.0;
                pen.push_box(x0);
                continue;
            }
            _ => {}
        }
        let Some(glyph) = font.glyph(ch, id) else {
            if !pen.doc.missing.contains(&ch) {
                pen.doc.missing.push(ch);
            }
            pen.doc.missing_at.push(i);
            if jitter_on {
                pen.cx += jit.space(Some(ch), id);
            } else {
                pen.cx += p.space_width + p.char_spacing;
            }
            pen.push_box(x0);
            prev = Some(ch);
            continue;
        };
        // Wrap before a glyph that would cross the right edge (never on an empty line).
        if p.page.is_some() && pen.cx > 0.0 && pen.cx + glyph.advance * s > pen.line_w {
            let b = if p.kinsoku { kinsoku::break_before(chars, pen.line_start, i) } else { i };
            if b < i {
                // Take chars b..i off this line and lay them again from the next one.
                pen.truncate_to(b);
                pen.newline(b, b);
                fresh_line = true;
                prev = Some(chars[b - 1]);
                next = b;
                continue;
            }
            pen.newline(i, i);
            if jitter_on {
                pen.cx = jit.line_start(id);
            }
        }
        let x0 = pen.cx;
        // `line_x + cx` is exactly `cx` on Free ruling (line_x = 0), keeping the golden bytes.
        let (cx, line_top) = (pen.line_x + pen.cx, pen.text_top);
        let t = tune(i);
        if !t.is_identity() {
            // 字符微调: drawn like a grid cell's char (same jitter and deformation), moved,
            // resized and re-rolled; the advance is unchanged, so neighbours stay put.
            draw_char(&mut pen, font, &mut jit, ch, id, cx, s, s, 0.0, t);
            pen.cx += glyph.advance * s + if jitter_on { jit.spacing(ch, id) } else { p.char_spacing };
        } else if !jitter_on && !deform_on {
            for stroke in &glyph.strokes {
                pen.page.strokes.push(
                    stroke
                        .iter()
                        .map(|&(gx, gy)| (ox + cx + gx * s, oy - (line_top + gy * s)))
                        .collect(),
                );
            }
            pen.cx += glyph.advance * s + p.char_spacing;
        } else {
            let voff = if jitter_on { jit.vertical(id, pen.cx, pen.line_w) } else { 0.0 };
            let mut drng = rng_for(p.style.seed, SALT_DEFORM, id);
            let strokes = p.deformation.apply(&glyph.strokes, is_cjk(ch), s, &mut drng);
            for stroke in &strokes {
                pen.page.strokes.push(
                    stroke
                        .iter()
                        .map(|&(gx, gy)| (ox + cx + gx * s, oy - (line_top + voff + gy * s)))
                        .collect(),
                );
            }
            pen.cx += glyph.advance * s + if jitter_on { jit.spacing(ch, id) } else { p.char_spacing };
        }
        pen.push_box(x0);
        prev = Some(ch);
    }
    pen.tag_strokes(chars.len().saturating_sub(1));
    pen.close_line(chars.len(), false);
    if table {
        pen.finish_cell(chars.len());
    }
    let mut doc = pen.doc;
    doc.pages.push(pen.page);
    doc.punct_issues = kinsoku::issues(chars, &doc.lines);
    doc
}

