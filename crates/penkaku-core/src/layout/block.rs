//! Objects in the text flow, drawn by the pen: inline tables (borders plus each cell's text laid
//! out as its own little document) and flowcharts. Both start on a fresh line; their strokes
//! are tagged to the [`OBJECT`] char except cell text, which keeps its own chars.

use super::pen::Pen;
use super::{Align, Block, CharBox, Document, EM_UNITS, Flow, LineInfo, OBJECT, PageBox, PageParams, Para, Ruling, TABLE_END, layout_pass, table_cells};
use crate::diagram::{self, mermaid};
use crate::font::fontset::Glyphs;
use crate::handwriting::{SALT_FRAME, rng_for};

type Pt = (f32, f32);

/// Lay out the object at char `i`. Returns the index after it and the number of `'\n'` it
/// swallowed, or `None` when it is malformed (a table without its end) and is skipped as text.
pub(super) fn lay(pen: &mut Pen, f: &Flow, font: &(impl Glyphs + ?Sized), i: usize, b: &Block) -> Option<(usize, usize)> {
    match b {
        Block::Table { cols } => {
            let end = (i + 1..f.chars.len()).find(|&k| matches!(f.chars[k], TABLE_END | OBJECT)).filter(|&k| f.chars[k] == TABLE_END)?;
            start_fresh(pen, i);
            lay_table(pen, f, font, i, end, cols);
            Some((end + 1, f.chars[i..end].iter().filter(|&&c| c == '\n').count()))
        }
        Block::Diagram { source } => {
            start_fresh(pen, i);
            lay_diagram(pen, f, font, i, source);
            Some((i + 1, 0))
        }
    }
}

/// End the current line unless it is empty, so the object starts on its own.
fn start_fresh(pen: &mut Pen, i: usize) {
    if i > pen.line_start {
        pen.newline(i, i);
    }
}

/// `s[r]`, clamped (per-char side tables may be shorter than the text).
fn part<T>(s: &[T], r: std::ops::Range<usize>) -> &[T] {
    let n = s.len();
    &s[r.start.min(n)..r.end.min(n)]
}

/// Line pitch of the ruling: Free uses the line height, ruled paper its row pitch.
fn pitch(pen: &Pen) -> f32 {
    match pen.p.ruling {
        Ruling::Rows { .. } => pen.row_h,
        _ => pen.p.line_height,
    }
}

/// Move to a place where `h` mm (`slots` ruled rows) fit below; returns the top (mm, y down).
/// Free: the current line top, or a new page. Ruled: the current row, or the next column/page.
fn make_room(pen: &mut Pen, h: f32, slots: usize) -> f32 {
    match pen.p.ruling {
        Ruling::Rows { columns, .. } => {
            let n = pen.rows.max(1) as usize;
            let r = pen.slot % n;
            if r > 0 && r + slots > n {
                pen.slot = (pen.slot / n + 1) * n;
                if pen.slot >= n * columns.max(1) as usize {
                    pen.new_page();
                }
                pen.enter_slot();
            }
            pen.row_top
        }
        _ => {
            if let Some(b) = pen.p.page
                && pen.line_top > 0.0
                && pen.line_top + h > b.height
            {
                pen.new_page();
                pen.enter_slot();
            }
            pen.line_top
        }
    }
}

/// After an object `bottom` mm down (its last ruled row: `last_slot`): the open line becomes the
/// object's last band, so the `'\n'` after it moves to the line below.
fn leave(pen: &mut Pen, top: f32, bottom: f32, last_slot: usize) {
    match pen.p.ruling {
        Ruling::Rows { .. } => {
            pen.slot = last_slot;
            pen.enter_slot();
        }
        _ => {
            let gap = EM_UNITS * pen.p.mm_per_unit * 0.4;
            pen.line_top = bottom + gap - pen.p.line_height;
            pen.row_top = top;
            pen.row_h = bottom - top;
        }
    }
    pen.line_stroke = pen.page.strokes.len();
}

/// Stable id for the `k`-th frame line or label char of the object whose anchor id is `id`;
/// the top bit keeps it clear of document ids.
fn sub_id(id: u64, k: u64) -> u64 {
    let mut z = id ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) | 1 << 63
}

/// A ruled polyline, wavering a little (bowed and wobbly, corners kept) with handwriting on.
fn waver(p: &PageParams, pts: &[Pt], id: u64) -> Vec<Pt> {
    if !p.style.enabled || pts.len() < 2 {
        return pts.to_vec();
    }
    let mut rng = rng_for(p.style.seed, SALT_FRAME, id);
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        if len < 1.0 {
            out.push(b);
            continue;
        }
        let (nx, ny) = (-dy / len, dx / len);
        let bow = (rng.unit() - 0.5) * 0.4;
        let n = (len / 4.0).ceil().max(1.0) as usize;
        for j in 1..=n {
            let t = j as f32 / n as f32;
            let off = if j == n { 0.0 } else { bow * (t * std::f32::consts::PI).sin() + (rng.unit() - 0.5) * 0.24 };
            out.push((a.0 + dx * t + nx * off, a.1 + dy * t + ny * off));
        }
    }
    out
}

/// A sub-document: chars `r` of the flow laid out alone at `mm_per_unit` × `k`, `width` mm wide
/// (∞ = no wrapping), lines `pitch` apart, no first-line indent, origin (0, 0). `r`: where the
/// chars sit in the flow (for their formats); `None` for text of our own (flowchart labels).
#[allow(clippy::too_many_arguments)]
fn sub_layout(pen: &Pen, f: &Flow, font: &(impl Glyphs + ?Sized), chars: &[char], ids: &[u64], r: Option<std::ops::Range<usize>>, k: f32, width: f32, pitch: f32) -> Document {
    let p = pen.p;
    let sp = PageParams {
        ruling: Ruling::Free,
        page: width.is_finite().then_some(PageBox { width, height: f32::INFINITY }),
        origin: (0.0, 0.0),
        line_height: pitch,
        mm_per_unit: p.mm_per_unit * k,
        ..p.clone()
    };
    let paras = vec![Para { align: Align::Left, first_indent: Some(0.0) }; chars.iter().filter(|&&c| c == '\n').count() + 1];
    let flow = match r {
        Some(r) => Flow {
            chars,
            ids,
            paras: &paras,
            joins: part(f.joins, r.clone()),
            tuning: part(f.tuning, r.clone()),
            styles: part(f.styles, r),
            deform_cache: f.deform_cache,
            blocks: &[],
        },
        None => Flow { chars, ids, paras: &paras, deform_cache: f.deform_cache, ..Flow::default() },
    };
    layout_pass(&flow, font, &sp, &[])
}

/// Copy a sub-document's page 0 into the pen: strokes moved by (dx, dy) (machine mm) and
/// tagged `first + k`; boxes and lines with char indices offset by `first`, lines moved by
/// `line_dy` (boxes and lines only when `with_text`).
fn graft(pen: &mut Pen, sub: Document, first: usize, dx: f32, dy: f32, line_dy: f32, tag: Option<usize>) {
    let base_line = pen.doc.lines.len();
    let page = pen.page_ix;
    let Some(sp) = sub.pages.into_iter().next() else { return };
    for (s, c) in sp.strokes.into_iter().zip(sp.stroke_chars.into_iter().chain(std::iter::repeat(0))) {
        pen.page.strokes.push(s.into_iter().map(|(x, y)| (x + dx, y + dy)).collect());
        pen.page.stroke_chars.push(tag.unwrap_or(first + c));
    }
    if tag.is_some() {
        return;
    }
    for b in sub.boxes {
        pen.doc.boxes.push(CharBox { page, line: base_line + b.line, x0: b.x0 + dx, x1: b.x1 + dx });
    }
    for l in sub.lines {
        pen.doc.lines.push(LineInfo { page, top: l.top + line_dy, bottom: l.bottom + line_dy, start: l.start + first, end: l.end + first });
    }
    for c in sub.missing {
        if !pen.doc.missing.contains(&c) {
            pen.doc.missing.push(c);
        }
    }
    pen.doc.missing_at.extend(sub.missing_at.into_iter().map(|k| k + first));
}

/// A zero-width caret stop at machine x on line `line`.
fn marker(pen: &mut Pen, x: f32, line: usize) {
    pen.doc.boxes.push(CharBox { page: pen.page_ix, line, x0: x, x1: x });
}

/// The part of a table on one page (and ruled column): page, left x (machine mm), row edges
/// (mm down from the origin, top first).
struct Part {
    page: usize,
    x0: f32,
    ys: Vec<f32>,
}

/// Write a part's frame: one line per row edge, one per column edge from its top to its bottom,
/// onto the page it belongs to, tagged to the anchor `i`.
fn frame(pen: &mut Pen, part: &Part, xs: &[f32], i: usize, id: u64, k: &mut u64) {
    let p = pen.p;
    let oy = p.origin.1;
    let (Some(&top), Some(&bottom)) = (part.ys.first(), part.ys.last()) else { return };
    let w = xs.last().copied().unwrap_or(0.0);
    let x0 = part.x0;
    let mut lines: Vec<Vec<Pt>> = part.ys.iter().map(|&y| vec![(x0, oy - y), (x0 + w, oy - y)]).collect();
    lines.extend(xs.iter().map(|&x| vec![(x0 + x, oy - top), (x0 + x, oy - bottom)]));
    let page = if part.page == pen.page_ix { &mut pen.page } else { &mut pen.doc.pages[part.page] };
    for l in lines {
        *k += 1;
        page.strokes.push(waver(p, &l, sub_id(id, *k)));
        page.stroke_chars.push(i);
    }
}

fn lay_table(pen: &mut Pen, f: &Flow, font: &(impl Glyphs + ?Sized), i: usize, end: usize, cols: &[f32]) {
    let p = pen.p;
    let (ox, oy) = p.origin;
    let pitch = pitch(pen);
    let em = EM_UNITS * p.mm_per_unit;
    let pad = em * 0.3;
    let ncols = cols.len().max(1);
    let mut widths: Vec<f32> = if cols.is_empty() { vec![40.0] } else { cols.iter().map(|w| w.max(2.0 * pad + 1.0)).collect() };
    let total: f32 = widths.iter().sum();
    if pen.line_w.is_finite() && total > pen.line_w {
        let k = pen.line_w / total;
        widths.iter_mut().for_each(|w| *w *= k);
    }
    let table_w: f32 = widths.iter().sum();
    let xs: Vec<f32> = std::iter::once(0.0)
        .chain(widths.iter().scan(0.0, |x, w| {
            *x += w;
            Some(*x)
        }))
        .collect();
    let xoff = match pen.align {
        _ if !pen.line_w.is_finite() => 0.0,
        Align::Center => ((pen.line_w - table_w) / 2.0).max(0.0),
        Align::Right => (pen.line_w - table_w).max(0.0),
        _ => 0.0,
    };

    let cells = table_cells(f.chars, i, end);
    let id = f.ids[i];
    // Each cell alone, wrapped to its column.
    let mut subs: Vec<Option<Document>> = cells
        .iter()
        .enumerate()
        .map(|(c, r)| Some(sub_layout(pen, f, font, &f.chars[r.clone()], &f.ids[r.clone()], Some(r.clone()), 1.0, widths[c % ncols] - 2.0 * pad, pitch)))
        .collect();

    let voff = (pitch - em) / 2.0;
    let mut part: Option<Part> = None;
    let mut frame_k = 0u64;
    let (mut top, mut bottom, mut last_slot) = (0.0, 0.0, pen.slot);
    for r in 0..cells.len().div_ceil(ncols) {
        let row = r * ncols..((r + 1) * ncols).min(cells.len());
        let lines = row.clone().map(|c| subs[c].as_ref().map_or(1, |d| d.lines.len().max(1))).max().unwrap_or(1);
        let h = lines as f32 * pitch;
        if r > 0 {
            // Below the previous row.
            match p.ruling {
                Ruling::Rows { columns, .. } => {
                    pen.slot = last_slot + 1;
                    if pen.slot >= pen.rows.max(1) as usize * columns.max(1) as usize {
                        pen.new_page();
                    }
                    pen.enter_slot();
                }
                _ => pen.line_top = bottom,
            }
        }
        top = make_room(pen, h, lines);
        let x0 = ox + pen.line_x + xoff;
        if part.as_ref().is_none_or(|pt| pt.page != pen.page_ix || pt.x0 != x0) {
            if let Some(pt) = part.take() {
                frame(pen, &pt, &xs, i, id, &mut frame_k);
            }
            part = Some(Part { page: pen.page_ix, x0, ys: vec![top] });
        }
        if r == 0 {
            // The anchor: a caret stop at the table's top-left corner, on a line of its own.
            marker(pen, x0, pen.doc.lines.len());
            pen.doc.lines.push(LineInfo { page: pen.page_ix, top: oy - top, bottom: oy - top, start: i, end: i });
        }
        for c in row.clone() {
            let cell = subs[c].take().unwrap_or_default();
            let rr = cells[c].clone();
            let dx = x0 + xs[c - row.start] + pad;
            let last_line = pen.doc.lines.len() + cell.lines.len().saturating_sub(1);
            let last_x = cell.boxes.last().map_or(dx, |b| b.x1 + dx);
            graft(pen, cell, rr.start, dx, oy - (top + voff), oy - top, None);
            // The cell break after the cell: a caret stop at its text's end.
            if rr.end < end {
                marker(pen, last_x, last_line);
            }
        }
        bottom = top + h;
        if let Some(pt) = part.as_mut() {
            pt.ys.push(bottom);
        }
        last_slot = match p.ruling {
            Ruling::Rows { .. } => pen.slot + lines - 1,
            _ => pen.slot,
        };
    }
    if let Some(pt) = part.take() {
        frame(pen, &pt, &xs, i, id, &mut frame_k);
    }
    // The end marker: a caret stop after the table, on the line the '\n' after it closes.
    let x_end = ox + pen.line_x + xoff + table_w;
    leave(pen, top, bottom, last_slot);
    marker(pen, x_end, pen.doc.lines.len());
    pen.line_start = end;
    pen.cx = xoff + table_w;
    pen.align = Align::Left;
}

fn lay_diagram(pen: &mut Pen, f: &Flow, font: &(impl Glyphs + ?Sized), i: usize, source: &str) {
    let p = pen.p;
    let (ox, oy) = p.origin;
    let g = match mermaid::parse(source) {
        Ok(g) => g,
        Err(e) => {
            pen.doc.block_errors.push((i, e.to_string()));
            let x = pen.cx;
            pen.push_box(x);
            return;
        }
    };
    let id = f.ids[i];
    let em = EM_UNITS * p.mm_per_unit;
    let lh = em * 1.3;
    let label_ids = |n: usize, k: usize| (0..n).map(|j| sub_id(id, ((k as u64) << 24) | j as u64)).collect::<Vec<u64>>();
    // Labels are numbered in draw order: node labels, then edge labels.
    let texts: Vec<&str> = g.nodes.iter().map(|n| n.label.as_str()).chain(g.edges.iter().filter_map(|e| e.label.as_deref())).collect();
    let lay_label = |pen: &Pen, text: &str, k: usize, scale: f32| {
        let chars: Vec<char> = text.chars().collect();
        sub_layout(pen, f, font, &chars, &label_ids(chars.len(), k), None, scale, f32::INFINITY, lh * scale)
    };
    let size_of = |d: &Document, scale: f32| {
        let (x0, x1) = d.boxes.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), bx| (a.min(bx.x0), b.max(bx.x1)));
        let w = if x0.is_finite() { x1 - x0 } else { 0.0 };
        (w, d.lines.len().max(1) as f32 * lh * scale - (lh - em) * scale, if x0.is_finite() { x0 } else { 0.0 })
    };
    let draw = |pen: &Pen, scale: f32| {
        let measure = |t: &str| {
            let k = texts.iter().position(|&x| x == t).unwrap_or(0);
            let (w, h, _) = size_of(&lay_label(pen, t, k, scale), scale);
            (w, h)
        };
        diagram::draw(&g, em * scale, &measure)
    };
    let avail_h = match p.ruling {
        Ruling::Rows { .. } => pen.rows.max(1) as f32 * pen.row_h,
        _ => p.page.map_or(f32::INFINITY, |b| b.height),
    };
    let mut scale = 1.0;
    let mut d = draw(pen, scale);
    let fit = (pen.line_w / d.size.0.max(1e-3)).min(avail_h / d.size.1.max(1e-3));
    if fit < 1.0 {
        scale = fit * 0.98;
        d = draw(pen, scale);
    }
    let slots = (d.size.1 / pitch(pen)).ceil().max(1.0) as usize;
    let top = make_room(pen, d.size.1, slots);
    let first_slot = pen.slot;
    let x0 = ox + pen.line_x;
    let stroke0 = pen.page.strokes.len();
    for (k, (s, w)) in d.strokes.iter().zip(&d.waver).enumerate() {
        let pts: Vec<Pt> = s.iter().map(|&(x, y)| (x0 + x, oy - (top + y))).collect();
        pen.page.strokes.push(if *w { waver(p, &pts, sub_id(id, k as u64 + 1)) } else { pts });
        pen.page.stroke_chars.push(i);
    }
    for l in &d.labels {
        let k = texts.iter().position(|&x| x == l.text).unwrap_or(0);
        let sub = lay_label(pen, &l.text, k, scale);
        let (w, h, minx) = size_of(&sub, scale);
        let dx = x0 + l.center.0 - w / 2.0 - minx;
        let dy = oy - (top + l.center.1 - h / 2.0);
        graft(pen, sub, i, dx, dy, 0.0, Some(i));
    }
    let bottom = top + d.size.1;
    let last_slot = first_slot + slots - 1;
    let align = pen.align;
    leave(pen, top, bottom, last_slot);
    // The diagram is one wide char on the line it opens: alignment shifts it like text.
    pen.line_stroke = stroke0;
    pen.align = align;
    pen.line_start = i;
    pen.cx = d.size.0;
    pen.doc.boxes.push(CharBox { page: pen.page_ix, line: pen.doc.lines.len(), x0, x1: x0 + d.size.0 });
}
