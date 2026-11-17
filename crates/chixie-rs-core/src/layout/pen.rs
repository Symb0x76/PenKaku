//! Line and page bookkeeping while laying out.

use super::{Align, CharBox, Tuning, Document, Guide, LineInfo, Page, PageBox, PageParams, Ruling};
use crate::font::fontset::Glyphs;
use crate::font::{Font, Glyph};
use crate::handwriting::{Jitter, SALT_DEFORM, SALT_ROWS, is_cjk, rng_for};

/// Pen state while laying out; owns the line/page bookkeeping.
pub(super) struct Pen<'p> {
    pub(super) p: &'p PageParams,
    pub(super) chars: &'p [char],
    /// Alignment of the paragraph the current line belongs to.
    pub(super) align: Align,
    /// Index into `page.strokes` of the current line's first stroke.
    pub(super) line_stroke: usize,
    /// Grid cell pitch (mm): aligned grid lines move by whole cells.
    pub(super) pitch: Option<f32>,
    pub(super) doc: Document,
    pub(super) page: Page,
    pub(super) page_ix: usize,
    pub(super) cx: f32,       // cursor x in mm, relative to the line's left edge
    pub(super) line_top: f32, // Free ruling: mm below origin for the current line (accumulated)
    pub(super) line_start: usize,
    /// Line slot on the current page (ruled / grid) and cell column (grid).
    pub(super) slot: usize,
    pub(super) col: u32,
    /// Row count of the current page (ruled / grid).
    pub(super) rows: u32,
    /// Current line geometry, mm relative to origin (x right, y down).
    pub(super) line_x: f32,
    pub(super) line_w: f32,
    pub(super) row_top: f32,
    pub(super) row_h: f32,
    /// Where glyph tops (glyph y = 0) go on this line.
    pub(super) text_top: f32,
    pub(super) em: f32,
}

impl Pen<'_> {
    pub(super) fn page_box(&self) -> PageBox {
        self.p.page.unwrap_or(PageBox { width: f32::INFINITY, height: f32::INFINITY })
    }

    /// Row count for page `k` (a random line count is drawn per page).
    pub(super) fn rows_for(&self, k: usize) -> u32 {
        match self.p.ruling {
            Ruling::Rows { rows: (lo, hi), .. } => {
                let lo = lo.max(1);
                if hi <= lo {
                    lo
                } else {
                    let r = rng_for(self.p.style.seed, SALT_ROWS, k as u64).next_u64();
                    lo + (r % u64::from(hi - lo + 1)) as u32
                }
            }
            Ruling::Grid { rows, .. } => rows.max(1),
            Ruling::Free => 0,
        }
    }

    /// Set the current line's geometry from the ruling and slot.
    pub(super) fn enter_slot(&mut self) {
        let b = self.page_box();
        match self.p.ruling {
            Ruling::Free => {
                (self.line_x, self.line_w) = (0.0, b.width);
                (self.row_top, self.row_h, self.text_top) = (self.line_top, self.p.line_height, self.line_top);
            }
            Ruling::Rows { columns, column_gap, band, .. } => {
                let n = self.rows as usize;
                let cols = columns.max(1) as f32;
                let pitch = b.height / n as f32;
                let col_w = (b.width - column_gap * (cols - 1.0)) / cols;
                let (c, r) = (self.slot / n, self.slot % n);
                self.line_x = c as f32 * (col_w + column_gap);
                self.line_w = col_w;
                self.row_top = r as f32 * pitch;
                self.row_h = pitch;
                // Sit on the rule: the glyph box (em) ends 0.2 em above the row's line. Captured
                // 2026-11-07 (我的笔记1, A4, 24 rows, size 100): text top 1.89 mm into a 10.29 mm row
                // on average; ChiXie3 adds a per-row spread of ≈ ±0.35 mm we do not model.
                let band = band.unwrap_or(0.0).min(pitch);
                let gap = 0.2 * self.em;
                self.text_top = (self.row_top + pitch - self.em - gap).max(self.row_top + band);
            }
            Ruling::Grid { cols, cell: (cw, ch), gap: (cg, rg), .. } => {
                self.line_x = 0.0;
                self.line_w = cols as f32 * (cw + cg);
                self.row_top = self.slot as f32 * (ch + rg);
                self.row_h = ch;
                self.text_top = self.row_top + (ch - self.em) / 2.0;
            }
        }
    }

    /// Ruling guides for the page being started.
    pub(super) fn page_guides(&mut self) {
        let b = self.page_box();
        let (ox, oy) = self.p.origin;
        let seg = |x0: f32, y0: f32, x1: f32, y1: f32, dashed: bool| Guide { a: (ox + x0, oy - y0), b: (ox + x1, oy - y1), dashed };
        let mut g = Vec::new();
        match self.p.ruling {
            Ruling::Free => {}
            Ruling::Rows { columns, column_gap, band, dashed, divider, .. } => {
                let n = self.rows;
                let cols = columns.max(1);
                let pitch = b.height / n as f32;
                let col_w = (b.width - column_gap * (cols as f32 - 1.0)) / cols as f32;
                for c in 0..cols {
                    let x0 = c as f32 * (col_w + column_gap);
                    for r in 0..=n {
                        let y = r as f32 * pitch;
                        g.push(seg(x0, y, x0 + col_w, y, false));
                        if let Some(band) = band
                            && r < n
                        {
                            let yb = y + band.min(pitch);
                            g.push(seg(x0, yb, x0 + col_w, yb, dashed));
                        }
                    }
                    if divider && c + 1 < cols {
                        let x = x0 + col_w + column_gap / 2.0;
                        g.push(seg(x, 0.0, x, b.height, false));
                    }
                }
            }
            Ruling::Grid { rows, cols, cell: (cw, ch), gap: (cg, rg) } => {
                for r in 0..rows {
                    let y0 = r as f32 * (ch + rg);
                    for c in 0..cols {
                        let x0 = c as f32 * (cw + cg);
                        let (x1, y1) = (x0 + cw, y0 + ch);
                        g.push(seg(x0, y0, x1, y0, false));
                        g.push(seg(x0, y1, x1, y1, false));
                        g.push(seg(x0, y0, x0, y1, false));
                        g.push(seg(x1, y0, x1, y1, false));
                    }
                }
            }
        }
        self.page.guides = g;
    }

    pub(super) fn new_page(&mut self) {
        self.doc.pages.push(std::mem::take(&mut self.page));
        self.page_ix += 1;
        self.line_top = 0.0;
        self.slot = 0;
        self.rows = self.rows_for(self.page_ix);
        self.page_guides();
    }

    /// Shift the finished line for its paragraph's alignment. Boxes pushed since the line started
    /// and strokes since `line_stroke` (all tagged by now) belong to this line. `wrapped`: the
    /// line ended by wrapping (justify only stretches those).
    fn align_line(&mut self, wrapped: bool) {
        if self.align == Align::Left {
            return;
        }
        let start = self.line_start;
        let n = self.doc.boxes.len();
        // Content ends at the last char that is not whitespace; trailing spaces hang.
        let Some(last) = (start..n).rev().find(|&k| !self.chars[k].is_whitespace()) else {
            return;
        };
        let left = self.p.origin.0 + self.line_x;
        let right = left + self.line_w;
        if !right.is_finite() {
            return;
        }
        let (x0, x1) = (self.doc.boxes[start].x0, self.doc.boxes[last].x1);
        let snap = |dx: f32| match self.pitch {
            Some(p) => (dx / p).floor() * p,
            None => dx,
        };
        let (base, per_gap) = match self.align {
            Align::Left => return,
            Align::Center => (snap((left + right - x0 - x1) / 2.0), 0.0),
            Align::Right => (snap(right - x1), 0.0),
            Align::Justify if wrapped && last > start && self.pitch.is_none() => (0.0, (right - x1) / (last - start) as f32),
            Align::Justify => return,
        };
        if base <= 0.0 && per_gap <= 0.0 {
            return;
        }
        let shift = |k: usize| base + per_gap * (k.min(last) - start) as f32;
        for (k, b) in self.doc.boxes.iter_mut().enumerate().skip(start) {
            let dx = shift(k);
            (b.x0, b.x1) = (b.x0 + dx, b.x1 + dx);
        }
        for (s, &c) in self.page.strokes[self.line_stroke..].iter_mut().zip(&self.page.stroke_chars[self.line_stroke..]) {
            let dx = shift(c);
            s.iter_mut().for_each(|pt| pt.0 += dx);
        }
    }

    pub(super) fn close_line(&mut self, end: usize, wrapped: bool) {
        self.align_line(wrapped);
        let (_, oy) = self.p.origin;
        self.doc.lines.push(LineInfo {
            page: self.page_ix,
            top: oy - self.row_top,
            bottom: oy - (self.row_top + self.row_h),
            start: self.line_start,
            end,
        });
    }

    /// Finish the line at caret index `end`; the next line starts at char `next`.
    pub(super) fn newline(&mut self, end: usize, next: usize) {
        self.close_line(end, next == end);
        self.cx = 0.0;
        self.col = 0;
        match self.p.ruling {
            Ruling::Free => {
                self.line_top += self.p.line_height;
                if let Some(b) = self.p.page
                    && self.line_top + self.p.line_height > b.height
                {
                    self.new_page();
                }
            }
            Ruling::Rows { columns, .. } => {
                self.slot += 1;
                if self.slot >= self.rows as usize * columns.max(1) as usize {
                    self.new_page();
                }
            }
            Ruling::Grid { .. } => {
                self.slot += 1;
                if self.slot >= self.rows as usize {
                    self.new_page();
                }
            }
        }
        self.enter_slot();
        self.line_start = next;
        self.line_stroke = self.page.strokes.len();
    }

    /// Drop everything laid for chars `b..` on the current line (boxes and tagged strokes), so
    /// they can be laid again on the next line.
    pub(super) fn truncate_to(&mut self, b: usize) {
        self.doc.boxes.truncate(b);
        self.doc.missing_at.retain(|&k| k < b);
        let keep = self.page.stroke_chars.iter().position(|&c| c >= b).unwrap_or(self.page.stroke_chars.len()).max(self.line_stroke);
        self.page.strokes.truncate(keep);
        self.page.stroke_chars.truncate(keep);
    }

    /// Attribute every untagged stroke on the current page to character `i`.
    pub(super) fn tag_strokes(&mut self, i: usize) {
        let n = self.page.strokes.len();
        self.page.stroke_chars.resize(n, i);
    }

    pub(super) fn push_box(&mut self, x0: f32) {
        let ox = self.p.origin.0 + self.line_x;
        let line = self.doc.lines.len();
        self.doc.boxes.push(CharBox { page: self.page_ix, line, x0: ox + x0, x1: ox + self.cx });
    }
}

/// Place one glyph's strokes with its left edge at `x` (mm from origin) on the current line,
/// glyph units scaled `sx` across and `sy` down.
fn draw_scaled(pen: &mut Pen, strokes: &[Vec<(f32, f32)>], x: f32, voff: f32, sx: f32, sy: f32) {
    let (ox, oy) = pen.p.origin;
    let top = pen.text_top;
    for stroke in strokes {
        pen.page.strokes.push(stroke.iter().map(|&(gx, gy)| (ox + x + gx * sx, oy - (top + voff + gy * sy))).collect());
    }
}

/// Draw `ch` at `x` with glyph units scaled `sx` across and `sy` down, `voff` mm lower than the
/// line's text top, plus its jitter and deformation as configured. A char the font lacks is
/// listed as missing and draws nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_char(pen: &mut Pen, font: &(impl Glyphs + ?Sized), jit: &mut Jitter, ch: char, id: u64, x: f32, sx: f32, sy: f32, voff: f32, t: Tuning) {
    // Spaces (a half space sharing a grid cell) draw nothing and are never missing.
    if ch.is_whitespace() {
        return;
    }
    let Some(glyph) = font.glyph(ch, id) else {
        if !pen.doc.missing.contains(&ch) {
            pen.doc.missing.push(ch);
        }
        // The char's box is pushed right after drawing: its index is the box count.
        pen.doc.missing_at.push(pen.doc.boxes.len());
        return;
    };
    let p = pen.p;
    // 字符微调: move, then scale about the glyph's bottom-left (the top moves, the foot stays).
    let (x, voff) = (x + t.dx_mm, voff + t.dy_mm + (1.0 - t.scale) * super::EM_UNITS * sy);
    let (sx, sy) = (sx * t.scale * t.width, sy * t.scale);
    // 变形 again: another deformation stream for this character.
    let deform_id = id.wrapping_add(u64::from(t.reroll).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let row_w = pen.line_w;
    let voff = voff + if p.style.enabled { jit.vertical(id, x, row_w) } else { 0.0 };
    if p.deformation.is_active() {
        let strokes = p.deformation.apply(&glyph.strokes, is_cjk(ch), sy, &mut rng_for(p.style.seed, SALT_DEFORM, deform_id));
        draw_scaled(pen, &strokes, x, voff, sx, sy);
    } else {
        draw_scaled(pen, &glyph.strokes, x, voff, sx, sy);
    }
}

/// The font's glyph for `ch`, or for its full/half-width counterpart when the font lacks it
/// (`,` ↔ `，`, `A` ↔ `Ａ`, space ↔ ideographic space). The text itself never changes.
pub fn glyph_for(font: &Font, ch: char) -> Option<&Glyph> {
    font.get(ch).or_else(|| width_counterpart(ch).and_then(|c| font.get(c)))
}

fn width_counterpart(ch: char) -> Option<char> {
    const SHIFT: u32 = 0xFF01 - 0x21;
    match ch as u32 {
        0x20 => Some('\u{3000}'),
        0x3000 => Some(' '),
        c @ 0x21..=0x7E => char::from_u32(c + SHIFT),
        c @ 0xFF01..=0xFF5E => char::from_u32(c - SHIFT),
        _ => None,
    }
}
