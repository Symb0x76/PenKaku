//! 字体几何标签: shape measures computed from a font's own strokes, so fonts can be filtered by
//! 方正 / 圆润, 端正 / 倾斜, 紧凑 / 舒展 without hand labels. Measured over common Chinese
//! characters the font has (every glyph when it has none of them).

use super::Font;
use crate::layout::EM_UNITS;

/// Common characters the measures sample (when the font has them).
const SAMPLE: &str = "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长";

/// Segments shorter than this share of the em are too short to tell a direction.
const MIN_SEG_EM: f32 = 0.1;
/// A pen lift shorter than this share of the em reads as a natural join.
const JOIN_GAP_EM: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct GeometryTags {
    /// Mean ink width ÷ height (1 = square; under 1 = tall and narrow).
    pub aspect: f32,
    /// Mean lean of the near-vertical strokes, degrees; positive leans right.
    pub slant_deg: f32,
    /// Mean ink box area ÷ em²: high = 舒展 (fills the cell), low = 紧凑.
    pub fill: f32,
    /// Mean stroke length per glyph ÷ em: how much ink a character takes (复杂度).
    pub ink_em: f32,
    /// Share of pen lifts inside a glyph shorter than 0.15 em: a natural 连笔 tendency.
    pub joined: f32,
    /// Glyphs measured.
    pub sampled: usize,
}

/// A lean under this many degrees reads as 端正.
pub const UPRIGHT_DEG: f32 = 3.0;

/// Which side of each axis a font picker keeps (`None` = either). 紧凑 / 舒展 and 修长 / 方正
/// are relative to the library's median font, so each side holds about half of it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagFilter {
    /// `true` 倾斜, `false` 端正.
    pub slanted: Option<bool>,
    /// `true` 紧凑, `false` 舒展.
    pub compact: Option<bool>,
    /// `true` 修长, `false` 方正.
    pub tall: Option<bool>,
}

impl TagFilter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl GeometryTags {
    /// Each measure's median over `all` (the yardstick for 紧凑 / 修长); `None` when empty.
    pub fn median(all: &[GeometryTags]) -> Option<GeometryTags> {
        let mid = |f: fn(&GeometryTags) -> f32| {
            let mut v: Vec<f32> = all.iter().map(f).collect();
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        (!all.is_empty()).then(|| GeometryTags {
            aspect: mid(|t| t.aspect),
            slant_deg: mid(|t| t.slant_deg),
            fill: mid(|t| t.fill),
            ink_em: mid(|t| t.ink_em),
            joined: mid(|t| t.joined),
            sampled: 0,
        })
    }

    pub fn slanted(&self) -> bool {
        self.slant_deg.abs() >= UPRIGHT_DEG
    }

    /// The font passes `f`, measured against the library's `mid`.
    pub fn matches(&self, f: TagFilter, mid: &GeometryTags) -> bool {
        f.slanted.is_none_or(|s| self.slanted() == s) && f.compact.is_none_or(|c| (self.fill < mid.fill) == c) && f.tall.is_none_or(|t| (self.aspect < mid.aspect) == t)
    }

    /// Short labels for a font card: 端正 / 倾斜, 紧凑 / 舒展, 方正 / 修长.
    pub fn labels(&self, mid: &GeometryTags) -> [&'static str; 3] {
        [
            if self.slanted() { "倾斜" } else { "端正" },
            if self.fill < mid.fill { "紧凑" } else { "舒展" },
            if self.aspect < mid.aspect { "修长" } else { "方正" },
        ]
    }
}

impl Font {
    /// [`GeometryTags`] of this font; `None` when no glyph has ink.
    pub fn geometry_tags(&self) -> Option<GeometryTags> {
        let mut picked: Vec<&super::Glyph> = SAMPLE.chars().filter_map(|c| self.get(c)).collect();
        if picked.is_empty() {
            picked = self.glyphs.values().collect();
        }
        let em = EM_UNITS;
        let (mut n, mut aspect, mut fill, mut ink) = (0usize, 0.0, 0.0, 0.0);
        let (mut lean, mut lean_w) = (0.0f32, 0.0f32);
        let (mut lifts, mut joins) = (0usize, 0usize);
        for g in picked {
            let pts = g.strokes.iter().flatten();
            let (x0, x1) = pts.clone().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
            let (y0, y1) = pts.fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
            let (w, h) = (x1 - x0, y1 - y0);
            if !(w.is_finite() && h.is_finite()) || h <= 0.0 {
                continue;
            }
            n += 1;
            aspect += w / h;
            fill += w * h / (em * em);
            for s in &g.strokes {
                for seg in s.windows(2) {
                    let (dx, dy) = (seg[1].0 - seg[0].0, seg[1].1 - seg[0].1);
                    let len = dx.hypot(dy);
                    ink += len / em;
                    // Near-vertical, pointed downward (y grows down): a right lean ends to the left.
                    if len >= MIN_SEG_EM * em && dy.abs() > 2.0 * dx.abs() {
                        let (dx, dy) = if dy < 0.0 { (-dx, -dy) } else { (dx, dy) };
                        lean += (-dx).atan2(dy).to_degrees() * len;
                        lean_w += len;
                    }
                }
            }
            for pair in g.strokes.windows(2) {
                if let (Some(a), Some(b)) = (pair[0].last(), pair[1].first()) {
                    lifts += 1;
                    joins += usize::from((b.0 - a.0).hypot(b.1 - a.1) < JOIN_GAP_EM * em);
                }
            }
        }
        (n > 0).then(|| {
            let k = n as f32;
            GeometryTags {
                aspect: aspect / k,
                slant_deg: if lean_w > 0.0 { lean / lean_w } else { 0.0 },
                fill: fill / k,
                ink_em: ink / k,
                joined: if lifts > 0 { joins as f32 / lifts as f32 } else { 0.0 },
                sampled: n,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::Glyph;
    use super::*;

    fn font(strokes: Vec<Vec<(f32, f32)>>) -> Font {
        Font { glyphs: [('的', Glyph { advance: 7.0, strokes })].into() }
    }

    #[test]
    fn upright_square_glyph() {
        // A 7×7 box drawn as four separate sides: square, upright, fills the em; each side
        // starts where the last ended, so every lift is 0 long and counts as a join.
        let sides = vec![vec![(0.0, 0.0), (7.0, 0.0)], vec![(7.0, 0.0), (7.0, 7.0)], vec![(7.0, 7.0), (0.0, 7.0)], vec![(0.0, 7.0), (0.0, 0.0)]];
        let t = font(sides).geometry_tags().unwrap();
        assert_eq!((t.aspect, t.slant_deg, t.fill, t.ink_em, t.joined, t.sampled), (1.0, 0.0, 1.0, 4.0, 1.0, 1));
    }

    #[test]
    fn filter_against_the_library_median() {
        let t = |aspect: f32, slant_deg: f32, fill: f32| GeometryTags { aspect, slant_deg, fill, ink_em: 3.0, joined: 0.1, sampled: 10 };
        let lib = [t(0.8, 0.5, 0.5), t(1.0, 6.0, 0.7), t(0.9, -4.0, 0.6)];
        let mid = GeometryTags::median(&lib).unwrap();
        assert_eq!((mid.aspect, mid.fill), (0.9, 0.6));
        let keep = |f: TagFilter| lib.iter().filter(|x| x.matches(f, &mid)).count();
        assert_eq!(keep(TagFilter::default()), 3);
        // Leaning either way counts as 倾斜.
        assert_eq!(keep(TagFilter { slanted: Some(true), ..Default::default() }), 2);
        assert_eq!(keep(TagFilter { slanted: Some(false), compact: Some(true), ..Default::default() }), 1);
        assert_eq!(keep(TagFilter { tall: Some(false), slanted: Some(false), ..Default::default() }), 0);
        assert_eq!(lib[0].labels(&mid), ["端正", "紧凑", "修长"]);
        assert_eq!(lib[1].labels(&mid), ["倾斜", "舒展", "方正"]);
        assert!(GeometryTags::median(&[]).is_none());
    }

    #[test]
    fn slant_and_lifts() {
        // Two strokes leaning right (bottom left of top) by atan(1/4), far apart.
        let lean = (1.0f32).atan2(4.0).to_degrees();
        let t = font(vec![vec![(2.0, 0.0), (1.0, 4.0)], vec![(6.0, 0.0), (5.0, 4.0)]]).geometry_tags().unwrap();
        assert!((t.slant_deg - lean).abs() < 1e-4, "{}", t.slant_deg);
        assert_eq!(t.joined, 0.0);
        // Drawn bottom-up it is the same lean; leaning left is negative.
        let up = font(vec![vec![(1.0, 4.0), (2.0, 0.0)]]).geometry_tags().unwrap();
        assert!((up.slant_deg - lean).abs() < 1e-4);
        let left = font(vec![vec![(1.0, 0.0), (2.0, 4.0)]]).geometry_tags().unwrap();
        assert!((left.slant_deg + lean).abs() < 1e-4);
        // No ink at all: no tags.
        assert_eq!(font(vec![]).geometry_tags(), None);
    }
}
