//! Text → positioned pen strokes in machine mm (`Document`).
//!
//! The preview and the G-code emitter (`gcode.rs`) both consume the same `Document`, so what is
//! shown is what is drawn. Glyph units are scaled to mm and Y is flipped (glyph y-down → machine
//! y-up). The page origin is the top-left corner, in machine mm.
//!
//! With every effect off, the arithmetic here keeps the exact float expressions and operation
//! order of the original `writing::text_to_gcode`, so its output stays byte-identical. The golden
//! test and the `legacy` oracle below check this; don't reorder the math.

use crate::deformation::Deformation;
use crate::font::Font;
use crate::handwriting::{HandwritingStyle, Jitter, SALT_DEFORM, is_cjk, rng_for};

mod pen;
#[cfg(test)]
pub(crate) mod legacy;
#[cfg(test)]
mod tests;

use pen::{Pen, draw_glyph};

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
    /// First stroke drawn for character `i` or any later one; `strokes.len()` if none.
    pub fn first_stroke_from(&self, i: usize) -> usize {
        self.stroke_chars.iter().position(|&c| c >= i).unwrap_or(self.strokes.len())
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
    /// `band`: dual-line mode, an upper band (mm) kept free above the writing line.
    Rows { rows: (u32, u32), columns: u32, column_gap: f32, band: Option<f32>, dashed: bool, divider: bool },
    /// Composition grid: one character per `cell` (w, h), `gap` (col, row) between cells.
    Grid { rows: u32, cols: u32, cell: (f32, f32), gap: (f32, f32) },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub pages: Vec<Page>,
    /// Characters with no glyph in the font (each skipped with a space-width advance), deduped,
    /// in first-seen order.
    pub missing: Vec<char>,
    /// One box per input character (glyphs, spaces, newlines, missing glyphs), in text order.
    pub boxes: Vec<CharBox>,
    /// Every line, in order; caret positions on a line are `start..=end` (char indices).
    pub lines: Vec<LineInfo>,
    /// Layout origin (machine mm): caret x on an empty line.
    pub origin: (f32, f32),
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
        }
    }
}

/// Lay out `text` with `font`, using character positions as ids (`0..n`). Unknown characters
/// advance like a space and are listed in `Document::missing`. See [`layout_ids`].
pub fn layout(text: &str, font: &Font, p: &PageParams) -> Document {
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
pub fn layout_ids(chars: &[char], ids: &[u64], font: &Font, p: &PageParams) -> Document {
    layout_doc(chars, ids, &[], font, p)
}

/// [`layout_ids`] with paragraph formatting: `paras[k]` applies to the k-th `'\n'`-separated
/// paragraph (missing entries are left-aligned with the default indent).
pub fn layout_doc(chars: &[char], ids: &[u64], paras: &[Para], font: &Font, p: &PageParams) -> Document {
    assert_eq!(chars.len(), ids.len(), "one id per character");
    let para = |k: usize| paras.get(k).copied().unwrap_or_default();
    let mut para_ix = 0;
    let s = p.mm_per_unit;
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
        doc: Document { origin: p.origin, ..Default::default() },
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
    };
    pen.rows = pen.rows_for(0);
    pen.enter_slot();
    pen.page_guides();
    let mut prev: Option<char> = None;
    // Jitter: the next placed character starts a line (and maybe a paragraph).
    let (mut fresh_line, mut fresh_paragraph) = (true, true);

    for (i, (&ch, &id)) in chars.iter().zip(ids).enumerate() {
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
        if let (Some((cols, cw, cg)), false) = (grid, ch == '\n') {
            // One character per cell, centred; spaces and missing glyphs take a cell too.
            if pen.col >= cols {
                pen.newline(i, i);
            }
            let cell_x = pen.col as f32 * (cw + cg);
            match (ch, font.get(ch)) {
                (' ', _) => {}
                (_, None) => {
                    if !pen.doc.missing.contains(&ch) {
                        pen.doc.missing.push(ch);
                    }
                }
                (_, Some(glyph)) => {
                    let voff = if jitter_on { jit.vertical(id) } else { 0.0 };
                    let x = cell_x + (cw - glyph.advance * s) / 2.0;
                    if deform_on {
                        let mut drng = rng_for(p.style.seed, SALT_DEFORM, id);
                        let strokes = p.deformation.apply(&glyph.strokes, is_cjk(ch), s, &mut drng);
                        draw_glyph(&mut pen, &strokes, x, voff, s);
                    } else {
                        draw_glyph(&mut pen, &glyph.strokes, x, voff, s);
                    }
                }
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
                pen.align = para(para_ix).align;
                (fresh_line, fresh_paragraph) = (true, true);
                prev = None;
                continue;
            }
            ' ' => {
                if jitter_on {
                    pen.cx += jit.space(prev, id);
                } else {
                    pen.cx += font.get(' ').map(|gl| gl.advance * s).unwrap_or(p.space_width);
                    pen.cx += p.char_spacing;
                }
                pen.push_box(x0);
                continue;
            }
            _ => {}
        }
        let Some(glyph) = font.get(ch) else {
            if !pen.doc.missing.contains(&ch) {
                pen.doc.missing.push(ch);
            }
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
            pen.newline(i, i);
            if jitter_on {
                pen.cx = jit.line_start(id);
            }
        }
        let x0 = pen.cx;
        // `line_x + cx` is exactly `cx` on Free ruling (line_x = 0), keeping the golden bytes.
        let (cx, line_top) = (pen.line_x + pen.cx, pen.text_top);
        if !jitter_on && !deform_on {
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
            let voff = if jitter_on { jit.vertical(id) } else { 0.0 };
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
    let mut doc = pen.doc;
    doc.pages.push(pen.page);
    doc
}

