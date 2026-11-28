//! 字体优化: clean-ups for fonts with uneven glyphs, applied when a font is loaded (the file is
//! never rewritten). Each fix only touches outliers measured against the font's own median, so a
//! well-made font is left exactly as it is.

use serde::{Deserialize, Serialize};

use super::{Font, Glyph, Point};
use crate::handwriting::is_cjk;

/// Which fixes to apply (all off by default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontFix {
    /// 统一字符尺寸: scale CJK glyphs much bigger or smaller than the rest to the median size.
    pub unify_size: bool,
    /// 重复轨迹优化: drop strokes that retrace an earlier stroke, and repeated points.
    pub dedupe: bool,
    /// 异常宽度优化: clamp abnormally wide advances.
    pub widths: bool,
    /// 反转笔顺: for a font stored back to front (last stroke first, each drawn from its end),
    /// every glyph's strokes in reverse order and direction. Unlike the others it touches every
    /// glyph, so it is set per font.
    pub reverse_strokes: bool,
}

impl FontFix {
    pub fn any(&self) -> bool {
        self.unify_size || self.dedupe || self.widths || self.reverse_strokes
    }

    /// Every fix that either side turns on.
    pub fn union(self, o: Self) -> Self {
        Self {
            unify_size: self.unify_size || o.unify_size,
            dedupe: self.dedupe || o.dedupe,
            widths: self.widths || o.widths,
            reverse_strokes: self.reverse_strokes || o.reverse_strokes,
        }
    }
}

/// `font` with the chosen fixes applied.
pub fn apply(font: &Font, fix: &FontFix) -> Font {
    let mut f = font.clone();
    // First, so the other fixes see the strokes in writing order.
    if fix.reverse_strokes {
        reverse_strokes(&mut f);
    }
    if fix.dedupe {
        dedupe_strokes(&mut f, 0.05);
    }
    if fix.unify_size {
        unify_size(&mut f);
    }
    if fix.widths {
        fix_widths(&mut f);
    }
    f
}

/// (min x, min y, max x, max y) of a glyph's points.
fn bbox(g: &Glyph) -> Option<(f32, f32, f32, f32)> {
    let mut it = g.strokes.iter().flatten();
    let &(x, y) = it.next()?;
    Some(it.fold((x, y, x, y), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y))))
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Glyphs of CJK characters that have strokes.
fn cjk(f: &Font) -> impl Iterator<Item = (&char, &Glyph)> {
    f.glyphs.iter().filter(|(c, g)| is_cjk(**c) && !g.strokes.is_empty())
}

/// Scale a CJK glyph whose size (larger bbox side) is outside 0.8–1.25 × the median to the median
/// size, about its own centre, then centre it where the median glyph sits.
pub fn unify_size(f: &mut Font) {
    let size = |b: (f32, f32, f32, f32)| (b.2 - b.0).max(b.3 - b.1);
    let boxes: Vec<(f32, f32, f32, f32)> = cjk(f).filter_map(|(_, g)| bbox(g)).collect();
    let (Some(m), Some(cx), Some(cy)) = (
        median(boxes.iter().map(|&b| size(b)).collect()),
        median(boxes.iter().map(|b| (b.0 + b.2) / 2.0).collect()),
        median(boxes.iter().map(|b| (b.1 + b.3) / 2.0).collect()),
    ) else {
        return;
    };
    for (c, g) in f.glyphs.iter_mut() {
        if !is_cjk(*c) {
            continue;
        }
        let Some(b) = bbox(g) else { continue };
        let sz = size(b);
        if sz <= 0.0 || (0.8..=1.25).contains(&(sz / m)) {
            continue;
        }
        let k = m / sz;
        let (gx, gy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
        for p in g.strokes.iter_mut().flatten() {
            *p = (cx + (p.0 - gx) * k, cy + (p.1 - gy) * k);
        }
    }
}

/// Remove repeated consecutive points, and strokes whose every point lies within `tol` of an
/// earlier stroke of the same glyph (the pen would draw the same line twice).
pub fn dedupe_strokes(f: &mut Font, tol: f32) {
    let near = |a: Point, b: Point| (a.0 - b.0).hypot(a.1 - b.1) <= tol;
    for g in f.glyphs.values_mut() {
        for s in g.strokes.iter_mut() {
            s.dedup_by(|a, b| near(*a, *b) && (a.0 - b.0).hypot(a.1 - b.1) < 1e-4);
        }
        let mut kept: Vec<Vec<Point>> = Vec::new();
        for s in std::mem::take(&mut g.strokes) {
            let retrace = s.len() > 1 && kept.iter().any(|k| s.iter().all(|&p| on_polyline(p, k, tol)));
            if !retrace {
                kept.push(s);
            }
        }
        g.strokes = kept;
    }
}

/// Every glyph's strokes in reverse order, each from its end to its start.
pub fn reverse_strokes(f: &mut Font) {
    for g in f.glyphs.values_mut() {
        g.strokes.reverse();
        g.strokes.iter_mut().for_each(|s| s.reverse());
    }
}

/// Whether `p` is within `tol` of the polyline `line`.
fn on_polyline(p: Point, line: &[Point], tol: f32) -> bool {
    if line.len() == 1 {
        return (p.0 - line[0].0).hypot(p.1 - line[0].1) <= tol;
    }
    line.windows(2).any(|w| {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy)) <= tol
    })
}

/// Clamp advances wider than 1.5 × the median CJK advance (or not positive) to the glyph's own
/// right edge plus the median gap between a CJK glyph's ink and its advance.
pub fn fix_widths(f: &mut Font) {
    let (Some(m), Some(gap)) = (
        median(cjk(f).map(|(_, g)| g.advance).collect()),
        median(cjk(f).filter_map(|(_, g)| Some(g.advance - bbox(g)?.2)).collect()),
    ) else {
        return;
    };
    for g in f.glyphs.values_mut() {
        if g.advance > 1.5 * m || g.advance <= 0.0 {
            g.advance = bbox(g).map_or(m, |b| (b.2 + gap.max(0.0)).max(0.1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f32, advance: f32) -> Glyph {
        Glyph { advance, strokes: vec![vec![(0.0, 0.0), (size, 0.0), (size, size)]] }
    }

    fn font() -> Font {
        let mut glyphs: std::collections::HashMap<char, Glyph> = "一二三四五".chars().map(|c| (c, square(6.0, 7.0))).collect();
        glyphs.insert('大', square(12.0, 7.0)); // twice the size
        glyphs.insert('宽', square(6.0, 20.0)); // abnormal advance
        glyphs.insert('重', Glyph { advance: 7.0, strokes: vec![vec![(0.0, 0.0), (6.0, 0.0)], vec![(1.0, 0.01), (5.0, 0.0)], vec![(0.0, 3.0), (0.0, 3.0), (6.0, 3.0)]] });
        Font { glyphs }
    }

    #[test]
    fn nothing_on_means_unchanged() {
        assert_eq!(apply(&font(), &FontFix::default()), font());
    }

    #[test]
    fn outsized_glyph_scales_to_the_median_box() {
        let f = apply(&font(), &FontFix { unify_size: true, ..Default::default() });
        let b = bbox(f.get('大').unwrap()).unwrap();
        assert!(((b.2 - b.0) - 6.0).abs() < 1e-4 && ((b.3 - b.1) - 6.0).abs() < 1e-4, "{b:?}");
        assert_eq!(f.get('一'), font().get('一'), "normal glyphs untouched");
    }

    #[test]
    fn retraced_strokes_and_repeated_points_go() {
        let f = apply(&font(), &FontFix { dedupe: true, ..Default::default() });
        let g = f.get('重').unwrap();
        assert_eq!(g.strokes, vec![vec![(0.0, 0.0), (6.0, 0.0)], vec![(0.0, 3.0), (6.0, 3.0)]]);
    }

    #[test]
    fn reversed_font_is_written_in_stroke_order() {
        // 十 stored back to front: the 竖 bottom-up first, then the 横 right to left.
        let stored = Font { glyphs: std::collections::HashMap::from([('十', Glyph { advance: 7.0, strokes: vec![vec![(3.0, 6.0), (3.0, 0.0)], vec![(6.0, 3.0), (3.0, 3.0), (0.0, 3.0)]] })]) };
        let fix = FontFix { reverse_strokes: true, ..Default::default() };
        let f = apply(&stored, &fix);
        assert_eq!(f.get('十').unwrap().strokes, vec![vec![(0.0, 3.0), (3.0, 3.0), (6.0, 3.0)], vec![(3.0, 0.0), (3.0, 6.0)]]);
        assert_eq!(apply(&f, &fix), stored, "reversing twice gives the font back");
    }

    #[test]
    fn abnormal_advance_is_clamped() {
        let f = apply(&font(), &FontFix { widths: true, ..Default::default() });
        // Median gap = 7 − 6 = 1, so the 20-unit advance becomes 6 + 1.
        assert!((f.get('宽').unwrap().advance - 7.0).abs() < 1e-4);
        assert_eq!(f.get('一').unwrap().advance, 7.0);
    }
}
