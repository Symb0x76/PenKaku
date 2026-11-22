//! Line and page bookkeeping while laying out.

use super::{Align, CELL_BREAK, CellBox, CellFit, CellInfo, CharBox, CharStyle, Document, EM_UNITS, Guide, LineInfo, Page, PageBox, PageParams, Ruling, SUB_DROP, SUPER_RAISE, Script, Tuning, VAlign};
use crate::font::fontset::Glyphs;
use crate::font::{Font, Glyph};
use crate::handwriting::{Jitter, SALT_DEFORM, SALT_ROWS, SALT_RULE, is_cjk, rng_for};

/// Ruled columns as (left x, width) in mm: `widths` when it names every column (scaled down if
/// they overrun the page), else `columns` equal columns `gap` apart.
pub(crate) fn column_spans(page_w: f32, columns: u32, gap: f32, widths: &[f32]) -> Vec<(f32, f32)> {
    let n = columns.max(1) as usize;
    let room = page_w - gap * (n - 1) as f32;
    let ws: Vec<f32> = if widths.len() == n && widths.iter().all(|w| *w > 0.0) {
        let sum: f32 = widths.iter().sum();
        let k = if sum > room { room / sum } else { 1.0 };
        widths.iter().map(|w| w * k).collect()
    } else {
        vec![room / n as f32; n]
    };
    let mut x = 0.0;
    ws.into_iter()
        .map(|w| {
            let span = (x, w);
            x += w + gap;
            span
        })
        .collect()
}

/// Pen state while laying out; owns the line/page bookkeeping.
pub(super) struct Pen<'p> {
    pub(super) p: &'p PageParams,
    pub(super) chars: &'p [char],
    pub(super) ids: &'p [u64],
    /// 字符格式 per char (missing = plain): underline / strike-through are drawn per line.
    pub(super) styles: &'p [Option<CharStyle>],
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
    /// mm per glyph unit right now (a table cell can have its own size).
    pub(super) scale: f32,
    /// Table forms: size / rows overrides per text segment (from the fitting passes).
    pub(super) fits: &'p [Option<CellFit>],
    /// Table forms: the cell being filled (index into the template page's cells), the text
    /// segment, where the cell's lines / strokes / chars start, and its first overflowing char.
    pub(super) cell: usize,
    pub(super) seg: usize,
    pub(super) cell_first_line: usize,
    pub(super) cell_first_stroke: usize,
    pub(super) cell_first_char: usize,
    pub(super) overflow_from: Option<usize>,
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
            Ruling::Table { ref pages } => pages[k % pages.len()].first().map_or(1, |c| c.rows.max(1)),
            Ruling::Free => 0,
        }
    }

    pub(super) fn is_table(&self) -> bool {
        matches!(self.p.ruling, Ruling::Table { .. })
    }

    /// The cell being filled.
    fn cell_box(&self) -> CellBox {
        match &self.p.ruling {
            Ruling::Table { pages } => pages[self.page_ix % pages.len()][self.cell],
            _ => CellBox::default(),
        }
    }

    /// Start filling the current cell with the text from char `first`.
    pub(super) fn start_cell(&mut self, first: usize) {
        let c = self.cell_box();
        let fit = self.fits.get(self.seg).copied().flatten();
        self.rows = fit.map_or(c.rows, |f| f.rows).max(1);
        self.scale = self.p.mm_per_unit * c.scale * fit.map_or(1.0, |f| f.scale);
        self.em = EM_UNITS * self.scale;
        self.align = c.align;
        (self.slot, self.col, self.cx) = (0, 0, 0.0);
        self.overflow_from = None;
        self.cell_first_line = self.doc.lines.len();
        self.cell_first_stroke = self.page.strokes.len();
        self.cell_first_char = first;
        self.enter_slot();
        self.line_start = first;
        self.line_stroke = self.page.strokes.len();
    }

    /// A cell break at char `end`: close this cell and start the next (on the next copy of the
    /// template page after the last one) with the text from `next`.
    pub(super) fn next_cell(&mut self, end: usize, next: usize) {
        self.close_line(end, false);
        self.finish_cell(end);
        self.cell += 1;
        self.seg += 1;
        let n = match &self.p.ruling {
            Ruling::Table { pages } => pages[self.page_ix % pages.len()].len(),
            _ => 1,
        };
        if self.cell >= n {
            self.cell = 0;
            self.new_page();
        }
        self.start_cell(next);
    }

    /// The cell's text (chars up to `end`) is laid: drop what overflowed, place the lines
    /// vertically, and record the cell.
    pub(super) fn finish_cell(&mut self, end: usize) {
        let c = self.cell_box();
        if let Some(f) = self.overflow_from
            && self.page.stroke_chars.len() == self.page.strokes.len()
        {
            // Never written: overflow would run below the cell.
            let keep: Vec<bool> = self.page.stroke_chars.iter().map(|&k| k < f).collect();
            let mut it = keep.iter();
            self.page.strokes.retain(|_| *it.next().unwrap_or(&true));
            self.page.stroke_chars.retain(|&k| k < f);
        }
        let used = (self.doc.lines.len() - self.cell_first_line).min(self.rows as usize) as f32;
        let free = (self.rows as f32 - used).max(0.0) * self.row_h;
        let dy = match c.valign {
            VAlign::Top => 0.0,
            VAlign::Middle => free / 2.0,
            VAlign::Bottom => free,
        };
        if dy > 0.0 && self.overflow_from.is_none() {
            let from = self.cell_first_stroke.min(self.page.strokes.len());
            for s in &mut self.page.strokes[from..] {
                s.iter_mut().for_each(|pt| pt.1 -= dy);
            }
            for l in &mut self.doc.lines[self.cell_first_line..] {
                (l.top, l.bottom) = (l.top - dy, l.bottom - dy);
            }
        }
        let (ox, oy) = self.p.origin;
        self.doc.cells.push(CellInfo {
            page: self.page_ix,
            cell: self.cell,
            seg: self.seg,
            start: self.cell_first_char,
            end,
            rect: (ox + c.x, oy - c.y, ox + c.x + c.w, oy - (c.y + c.h)),
            fit: CellFit { scale: self.scale / self.p.mm_per_unit, rows: self.rows },
            overflow_from: self.overflow_from,
        });
    }

    /// Set the current line's geometry from the ruling and slot.
    pub(super) fn enter_slot(&mut self) {
        let b = self.page_box();
        match self.p.ruling {
            Ruling::Free => {
                (self.line_x, self.line_w) = (0.0, b.width);
                (self.row_top, self.row_h, self.text_top) = (self.line_top, self.p.line_height, self.line_top);
            }
            Ruling::Rows { columns, column_gap, ref widths, band, upper, .. } => {
                let n = self.rows as usize;
                let pitch = b.height / n as f32;
                let (c, r) = (self.slot / n, self.slot % n);
                (self.line_x, self.line_w) = column_spans(b.width, columns, column_gap, widths)[c];
                self.row_top = r as f32 * pitch;
                self.row_h = pitch;
                // Sit on the rule: the glyph box (em) ends 0.2 em above the row's line. Captured
                // 2026-11-07 (我的笔记1, A4, 24 rows, size 100): text top 1.89 mm into a 10.29 mm row
                // on average; ChiXie3 adds a per-row spread of ≈ ±0.35 mm we do not model.
                let band = band.unwrap_or(0.0).min(pitch);
                let gap = 0.2 * self.em;
                self.text_top = if upper && band > 0.0 {
                    // Upper band: sit on the band's line the same way.
                    (self.row_top + band - self.em - gap).max(self.row_top)
                } else {
                    (self.row_top + pitch - self.em - gap).max(self.row_top + band)
                };
            }
            Ruling::Table { .. } => {
                let c = self.cell_box();
                let (pl, pt, pr, pb) = c.padding;
                let pitch = ((c.h - pt - pb) / self.rows.max(1) as f32).max(0.1);
                self.line_x = c.x + pl;
                self.line_w = (c.w - pl - pr).max(0.1);
                // Rows past the last are the overflow (laid out, never written).
                self.row_top = c.y + pt + self.slot as f32 * pitch;
                self.row_h = pitch;
                // Centred in each writing row, like a grid cell. ChiXie3 capture 2026-11-18 (2×2
                // form, 12 mm rows, size 100): glyph tops 2.3–3.0 mm into the row (centred: 2.5).
                self.text_top = self.row_top + (pitch - self.em) / 2.0;
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
            Ruling::Rows { columns, column_gap, ref widths, band, dashed, divider, .. } => {
                let n = self.rows;
                let pitch = b.height / n as f32;
                let spans = column_spans(b.width, columns, column_gap, widths);
                let cols = spans.len();
                for (c, &(x0, col_w)) in spans.iter().enumerate() {
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
            Ruling::Table { ref pages } => {
                // Cell borders; a border shared by two cells is drawn once.
                for c in &pages[self.page_ix % pages.len()] {
                    let (x0, y0, x1, y1) = (c.x, c.y, c.x + c.w, c.y + c.h);
                    for gd in [seg(x0, y0, x1, y0, false), seg(x0, y1, x1, y1, false), seg(x0, y0, x0, y1, false), seg(x1, y0, x1, y1, false)] {
                        let same = |a: &Guide| (a.a == gd.a && a.b == gd.b) || (a.a == gd.b && a.b == gd.a);
                        if !g.iter().any(same) {
                            g.push(gd);
                        }
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
        self.rule_strokes();
        let (_, oy) = self.p.origin;
        self.doc.lines.push(LineInfo {
            page: self.page_ix,
            top: oy - self.row_top,
            bottom: oy - (self.row_top + self.row_h),
            start: self.line_start,
            end,
        });
    }

    /// 下划线 / 删除线 on the line being closed: one hand-drawn stroke per run of chars that has
    /// it, at the glyph foot / mid-height, written like ink and tagged to the run's last char.
    fn rule_strokes(&mut self) {
        type Pick = fn(&CharStyle) -> bool;
        if self.styles.iter().all(|s| s.is_none_or(|s| !s.underline && !s.strike)) {
            return;
        }
        let li = self.doc.lines.len();
        let n = self.doc.boxes.len();
        let on = |k: usize, f: Pick| {
            self.styles.get(k).copied().flatten().is_some_and(|s| f(&s)) && self.doc.boxes[k].line == li && !matches!(self.chars[k], '\n' | CELL_BREAK)
        };
        let oy = self.p.origin.1;
        let mut add = Vec::new();
        let picks: [(Pick, f32); 2] = [(|s| s.underline, 1.12), (|s| s.strike, 0.55)];
        for (f, depth) in picks {
            let y = oy - (self.text_top + depth * self.em);
            let mut k = self.line_start.min(n);
            while k < n {
                if !on(k, f) {
                    k += 1;
                    continue;
                }
                let first = k;
                while k + 1 < n && on(k + 1, f) {
                    k += 1;
                }
                let (x0, x1) = (self.doc.boxes[first].x0, self.doc.boxes[k].x1);
                add.push((self.rule_line(x0, x1, y, self.ids.get(k).copied().unwrap_or(k as u64)), k));
                k += 1;
            }
        }
        for (s, k) in add {
            self.page.strokes.push(s);
            self.page.stroke_chars.push(k);
        }
    }

    /// A ruled stroke from `x0` to `x1` at `y` (machine mm); wavers a little with handwriting on.
    fn rule_line(&self, x0: f32, x1: f32, y: f32, id: u64) -> Vec<(f32, f32)> {
        if !self.p.style.enabled || x1 - x0 < 1.0 {
            return vec![(x0, y), (x1, y)];
        }
        let mut rng = rng_for(self.p.style.seed, SALT_RULE, id);
        let tilt = (rng.unit() - 0.5) * 0.4;
        let n = ((x1 - x0) / 4.0).ceil().max(2.0) as usize;
        (0..=n)
            .map(|j| {
                let t = j as f32 / n as f32;
                let wobble = if j == 0 || j == n { 0.0 } else { (rng.unit() - 0.5) * 0.24 };
                (x0 + (x1 - x0) * t, y + tilt * (t - 0.5) + wobble)
            })
            .collect()
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
            // Within the cell; rows past the last one overflow (see `push_box`).
            Ruling::Table { .. } => self.slot += 1,
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
        let k = self.doc.boxes.len();
        if self.slot >= self.rows as usize
            && self.overflow_from.is_none()
            && self.is_table()
            && self.chars.get(k).is_some_and(|&c| !c.is_whitespace() && c != CELL_BREAK)
        {
            self.overflow_from = Some(k);
        }
        self.doc.boxes.push(CharBox { page: self.page_ix, line, x0: ox + x0, x1: ox + self.cx });
    }
}

/// Place one glyph's strokes with its left edge at `x` (mm from origin) on the current line,
/// glyph units scaled `sx` across and `sy` down; `shear` leans it about its foot.
#[allow(clippy::too_many_arguments)]
fn draw_scaled(pen: &mut Pen, strokes: &[Vec<(f32, f32)>], x: f32, voff: f32, sx: f32, sy: f32, shear: f32) {
    let (ox, oy) = pen.p.origin;
    let top = pen.text_top;
    for stroke in strokes {
        let pts = if shear == 0.0 {
            stroke.iter().map(|&(gx, gy)| (ox + x + gx * sx, oy - (top + voff + gy * sy))).collect()
        } else {
            stroke.iter().map(|&(gx, gy)| (ox + x + gx * sx + shear * (EM_UNITS - gy) * sy, oy - (top + voff + gy * sy))).collect()
        };
        pen.page.strokes.push(pts);
    }
}

/// Draw `ch` at `x` with glyph units scaled `sx` across and `sy` down, `voff` mm lower than the
/// line's text top, plus its jitter and deformation as configured, its 字符微调 `t` and its
/// 字符格式 `st` (size, script, tilt; the foot stays on the line). A char the font lacks is
/// listed as missing and draws nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_char(pen: &mut Pen, font: &(impl Glyphs + ?Sized), jit: &mut Jitter, ch: char, id: u64, x: f32, sx: f32, sy: f32, voff: f32, t: Tuning, st: CharStyle) {
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
    let (sx, sy, voff) = if st.is_plain() {
        (sx, sy, voff)
    } else {
        let (f, em) = (st.factor(), EM_UNITS * sy);
        let lift = match st.script {
            Script::None => 0.0,
            Script::Super => -SUPER_RAISE * em,
            Script::Sub => SUB_DROP * em,
        };
        (sx * f, sy * f, voff + (1.0 - f) * em + lift)
    };
    // 字符微调: move, then scale about the glyph's bottom-left (the top moves, the foot stays).
    let (x, voff) = (x + t.dx_mm, voff + t.dy_mm + (1.0 - t.scale) * super::EM_UNITS * sy);
    let (sx, sy) = (sx * t.scale * t.width, sy * t.scale);
    // 变形 again: another deformation stream for this character.
    let deform_id = id.wrapping_add(u64::from(t.reroll).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let row_w = pen.line_w;
    let voff = voff + if p.style.enabled { jit.vertical(id, x, row_w) } else { 0.0 };
    if p.deformation.is_active() {
        let strokes = p.deformation.apply(&glyph.strokes, is_cjk(ch), sy, &mut rng_for(p.style.seed, SALT_DEFORM, deform_id));
        draw_scaled(pen, &strokes, x, voff, sx, sy, st.shear());
    } else {
        draw_scaled(pen, &glyph.strokes, x, voff, sx, sy, st.shear());
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
