//! 排版体检: checks on a laid-out [`Document`] measured from the ink (the strokes tagged to each
//! character), not from advances, so jitter and deformation count.

use super::Document;

/// Ink past a line's right edge by more than this (mm) is an overflow; glyphs may overhang
/// their advance a little.
const OVERFLOW_TOL: f32 = 0.5;

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
