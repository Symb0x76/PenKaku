//! Line and page bookkeeping while laying out.

use super::{Document, Guide, Page, PageBox, PageParams, Ruling, LineInfo, CharBox};
use crate::handwriting::{SALT_ROWS, rng_for};

/// Pen state while laying out; owns the line/page bookkeeping.
pub(super) struct Pen<'p> {
    pub(super) p: &'p PageParams,
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

    pub(super) fn close_line(&mut self, end: usize) {
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
        self.close_line(end);
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

/// Place one glyph's strokes with its left edge at `x` (mm from origin) on the current line.
pub(super) fn draw_glyph(pen: &mut Pen, strokes: &[Vec<(f32, f32)>], x: f32, voff: f32, s: f32) {
    let (ox, oy) = pen.p.origin;
    let top = pen.text_top;
    for stroke in strokes {
        pen.page.strokes.push(stroke.iter().map(|&(gx, gy)| (ox + x + gx * s, oy - (top + voff + gy * s))).collect());
    }
}
