//! 排版体检: checks on a laid-out [`Document`] measured from the ink (the strokes tagged to each
//! character), not from advances, so jitter and deformation count. Also the batch fixes
//! 收紧大空隙 and 基线拉平, returned as per-char moves for 字符微调.
//!
//! A 下划线 / 删除线 stroke is tagged to its run's last char, so that char's ink spans the run:
//! underlined text can read as overlapping.

use std::ops::Range;

use super::{Document, OBJECT, TABLE_END};

/// Ink past a line's right edge by more than this (mm) is an overflow; glyphs may overhang
/// their advance a little.
const OVERFLOW_TOL: f32 = 0.5;
/// Ink of two neighbours overlapping by more than this (mm) is a collision.
const COLLIDE_TOL: f32 = 0.3;
/// A gap between two written letters wider than this share of the em is too wide. Glyph side
/// bearings plus the usual 0.6–1.1 mm spacing come to roughly 0.3 em.
pub const MAX_GAP_EM: f32 = 0.5;
/// 收紧 brings a wide gap down to the line's typical gap, or this share of the em.
const FALLBACK_GAP_EM: f32 = 0.25;
/// 基线拉平 only moves chars at least this tall (share of the em): short glyphs (一, 。, a)
/// sit where they sit by design.
const TALL_EM: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issue {
    /// The gap before `at` is too wide.
    Gap,
    /// `at` runs past its line's right edge (or is the first char of a table cell's overflow).
    Overflow,
    /// `at` overlaps its left neighbour or a char on the next line.
    Collision,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anomaly {
    pub kind: Issue,
    /// Char index.
    pub at: usize,
    /// Index into `doc.lines`.
    pub line: usize,
    /// How much: the gap's width, the overhang, or the overlap (mm).
    pub mm: f32,
}

/// A char that writes something (not a space, break or object marker).
fn writable(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && c != OBJECT && c != TABLE_END
}

/// Gap checks compare letters only: punctuation is mostly blank on one side.
fn letter(c: char) -> bool {
    c.is_alphanumeric()
}

/// The gap between neighbours `k - 1` and `k` (same line, both letters with ink), if any.
fn gap_before(chars: &[char], ink: &[Option<Ink>], doc: &Document, k: usize) -> Option<f32> {
    let (a, b) = (ink.get(k - 1).copied().flatten()?, ink.get(k).copied().flatten()?);
    let same_line = doc.boxes.get(k - 1)?.line == doc.boxes.get(k)?.line;
    (same_line && letter(chars[k - 1]) && letter(chars[k])).then_some(b.x0 - a.x1)
}

/// Every wide gap, overflow and collision, in text order; `em_mm` = the body text's em.
pub fn inspect(doc: &Document, chars: &[char], em_mm: f32) -> Vec<Anomaly> {
    let n = chars.len().min(doc.boxes.len());
    let ink = ink_boxes(doc, n);
    let line = |k: usize| doc.boxes[k].line;
    let mut out = Vec::new();
    // On a grid every char is centred in its cell: gaps are the cells', not the hand's.
    for k in (1..n).filter(|_| !doc.grid) {
        if let Some(g) = gap_before(chars, &ink, doc, k)
            && g > MAX_GAP_EM * em_mm
        {
            out.push(Anomaly { kind: Issue::Gap, at: k, line: line(k), mm: g });
        }
    }
    for l in overflow_lines(doc, n) {
        let li = doc.lines[l];
        let past = (li.start..li.end.min(n)).find(|&k| writable(chars[k]) && ink[k].is_some_and(|b| b.x1 > li.right + OVERFLOW_TOL));
        let at = past.or_else(|| doc.cells.iter().filter_map(|c| c.overflow_from).find(|&k| k < n && line(k) == l));
        if let Some(at) = at {
            out.push(Anomaly { kind: Issue::Overflow, at, line: l, mm: ink[at].map_or(0.0, |b| (b.x1 - li.right).max(0.0)) });
        }
    }
    // Neighbours on a line, then each char against the next line's chars on the same page.
    let inked: Vec<usize> = (0..n).filter(|&k| writable(chars[k]) && ink[k].is_some()).collect();
    let overlap = |a: Ink, b: Ink| ((a.x1.min(b.x1) - a.x0.max(b.x0)), (a.top.min(b.top) - a.bottom.max(b.bottom)));
    for w in inked.windows(2) {
        let (a, b) = (ink[w[0]].unwrap(), ink[w[1]].unwrap());
        let (ox, oy) = overlap(a, b);
        if line(w[0]) == line(w[1]) && w[1] == w[0] + 1 && ox > COLLIDE_TOL && oy > 0.0 {
            out.push(Anomaly { kind: Issue::Collision, at: w[1], line: line(w[1]), mm: ox });
        }
    }
    for (li, pair) in doc.lines.windows(2).enumerate() {
        let (up, down) = (pair[0], pair[1]);
        if up.page != down.page {
            continue;
        }
        let of = |l: super::LineInfo| inked.iter().copied().filter(move |&k| k >= l.start && k < l.end.min(n));
        for b in of(down) {
            let hit = of(up).map(|a| overlap(ink[a].unwrap(), ink[b].unwrap())).filter(|&(ox, oy)| ox > 0.0 && oy > COLLIDE_TOL).map(|o| o.1).fold(0.0, f32::max);
            if hit > 0.0 {
                out.push(Anomaly { kind: Issue::Collision, at: b, line: li + 1, mm: hit });
            }
        }
    }
    out.sort_by_key(|a| (a.at, a.kind as u8));
    out.dedup_by_key(|a| (a.at, a.kind));
    out
}

/// The lines (indices into `doc.lines`) holding any char of `sel`, or every line when `sel` is
/// empty: what a batch fix works on.
pub fn lines_of(doc: &Document, sel: Range<usize>) -> Range<usize> {
    if sel.is_empty() {
        return 0..doc.lines.len();
    }
    doc.line_of(sel.start)..doc.line_of(sel.end.saturating_sub(1).max(sel.start)) + 1
}

/// 收紧大空隙: each wide gap on `lines` comes down to the line's typical gap, by moving every
/// char after it left. Returns (char, dx mm) moves for 字符微调 (advances stay, so moving one
/// char never moves its neighbours: the shift is carried along the line). Nothing on a grid.
pub fn tighten(doc: &Document, chars: &[char], lines: Range<usize>, em_mm: f32) -> Vec<(usize, f32)> {
    if doc.grid {
        return Vec::new();
    }
    let n = chars.len().min(doc.boxes.len());
    let ink = ink_boxes(doc, n);
    let max = MAX_GAP_EM * em_mm;
    let mut out = Vec::new();
    for l in lines.filter_map(|i| doc.lines.get(i).copied()) {
        let span = l.start.max(1)..l.end.min(n);
        let mut gaps: Vec<f32> = span.clone().filter_map(|k| gap_before(chars, &ink, doc, k)).filter(|&g| g <= max).collect();
        gaps.sort_by(f32::total_cmp);
        let target = gaps.get(gaps.len() / 2).copied().unwrap_or(FALLBACK_GAP_EM * em_mm);
        let mut shift = 0.0;
        for k in span {
            if let Some(g) = gap_before(chars, &ink, doc, k).filter(|&g| g > max) {
                shift += g - target;
            }
            if shift > 0.0 && writable(chars[k]) {
                out.push((k, -shift));
            }
        }
    }
    out
}

/// 基线拉平: on `lines`, pull each tall char's ink centre toward the line's median centre by
/// `strength` (0 = no change, 1 = all the way). Returns (char, dy mm) moves for 字符微调
/// (positive = down).
pub fn align_baseline(doc: &Document, chars: &[char], lines: Range<usize>, strength: f32, em_mm: f32) -> Vec<(usize, f32)> {
    let n = chars.len().min(doc.boxes.len());
    let ink = ink_boxes(doc, n);
    let strength = strength.clamp(0.0, 1.0);
    let mut out = Vec::new();
    for l in lines.filter_map(|i| doc.lines.get(i).copied()) {
        let tall: Vec<(usize, f32)> = (l.start..l.end.min(n))
            .filter(|&k| letter(chars[k]))
            .filter_map(|k| ink[k].filter(|b| b.top - b.bottom >= TALL_EM * em_mm).map(|b| (k, (b.top + b.bottom) / 2.0)))
            .collect();
        let mut ys: Vec<f32> = tall.iter().map(|t| t.1).collect();
        ys.sort_by(f32::total_cmp);
        let Some(&mid) = ys.get(ys.len() / 2) else { continue };
        out.extend(tall.into_iter().map(|(k, y)| (k, (y - mid) * strength)).filter(|m| m.1.abs() > 0.01));
    }
    out
}

/// Ink extents of one character (machine mm, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink {
    pub x0: f32,
    pub x1: f32,
    pub bottom: f32,
    pub top: f32,
}

/// Each of the first `n` characters' ink, from the strokes tagged to it (`None`: no ink).
pub fn ink_boxes(doc: &Document, n: usize) -> Vec<Option<Ink>> {
    let mut out: Vec<Option<Ink>> = vec![None; n];
    for page in &doc.pages {
        for (s, &c) in page.strokes.iter().zip(&page.stroke_chars) {
            let Some(slot) = out.get_mut(c) else { continue };
            for &(x, y) in s {
                let b = slot.get_or_insert(Ink { x0: x, x1: x, bottom: y, top: y });
                (b.x0, b.x1, b.bottom, b.top) = (b.x0.min(x), b.x1.max(x), b.bottom.min(y), b.top.max(y));
            }
        }
    }
    out
}

/// Lines (indices into `doc.lines`) whose ink runs past their right edge, plus the lines of
/// table cells whose text does not fit; `n` = the laid-out text's length. Sorted.
pub fn overflow_lines(doc: &Document, n: usize) -> Vec<usize> {
    let ink = ink_boxes(doc, n);
    let mut v: Vec<usize> = doc
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.right.is_finite() && (l.start..l.end.min(n)).any(|k| ink[k].is_some_and(|b| b.x1 > l.right + OVERFLOW_TOL)))
        .map(|(i, _)| i)
        .collect();
    v.extend(doc.overflowing().filter_map(|c| c.overflow_from).map(|k| doc.line_of(k)));
    v.sort_unstable();
    v.dedup();
    v
}
