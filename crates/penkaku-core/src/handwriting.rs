//! Handwriting jitter: per-character spacing, space width, vertical offset and per-line start
//! offset, drawn from a seeded RNG so the same seed always produces the same `Document`.
//!
//! Semantics come from ChiXie3 captures (docs/protocol.md, "Handwriting effects" and
//! "Multi-line layout"):
//! - every value is absolute mm and does not scale with font size;
//! - values are drawn per character, in text order, when the text is laid out;
//! - the line start offset applies to every line, not only the first;
//! - no first-line indent was observed in the default mode, so it is off unless set.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::deformation::Deformation;
use crate::{Error, Result};

/// Inclusive `[min, max]` range in mm (or the field's documented unit). Named after ChiXie3's
/// `min_*` / `max_*` config keys.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub min: f32,
    pub max: f32,
}

impl Span {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn contains(&self, v: f32) -> bool {
        v >= self.min && v <= self.max
    }

    pub fn sample(&self, rng: &mut Rng) -> f32 {
        self.min + (self.max - self.min) * rng.unit()
    }
}

/// SplitMix64: tiny, fast and stable across platforms and versions, which matters because the
/// seed is persisted and must reproduce the same page later.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1]` (both ends reachable within f32 precision).
    pub fn unit(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / ((1u64 << 24) - 1) as f32
    }
}

/// How the vertical offset evolves along a row. Variant meanings beyond `Independent` are
/// best guesses: ChiXie3's `row_offset_style` value 1 was not distinguishable in captures. VERIFY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowOffsetStyle {
    /// Each glyph draws its own offset from `vertical_offset` (matches captures).
    #[default]
    Independent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandwritingStyle {
    pub enabled: bool,
    pub seed: u64,
    /// Extra gap after each CJK / non-CJK glyph, mm.
    pub char_spacing_zh: Span,
    pub char_spacing_en: Span,
    /// Width of a space after CJK / non-CJK context, mm.
    pub space_width_zh: Span,
    pub space_width_en: Span,
    /// Downward offset of each glyph from its line, mm.
    pub vertical_offset: Span,
    pub row_offset_style: RowOffsetStyle,
    /// Smooth the vertical offset against the previous glyph (random walk clamped to
    /// `vertical_offset`) instead of drawing independently. Best guess, VERIFY.
    pub intelligent_vertical_offset: bool,
    /// Pull the walk back toward the middle of the range each glyph. Only meaningful with
    /// `intelligent_vertical_offset`. Best guess, VERIFY.
    pub random_correct_offset: bool,
    /// X shift of the first glyph of every line, mm (ChiXie3 `first_text_not_align_range`, which
    /// captures show applies per line).
    pub line_start_offset: Span,
    /// Indent of a paragraph's first line, mm. `None` = not applied (ChiXie3 default mode).
    pub first_line_indent: Option<Span>,
    /// 中英文分开设置: Latin letters and digits can differ from Chinese (not in ChiXie3).
    pub english: ScriptStyle,
    pub number: ScriptStyle,
    /// 连笔 (not in ChiXie3). Its own switch, independent of `enabled`.
    pub cursive: Cursive,
    /// 随机轻重: each char's pen pressure varies by up to ± this many 轻重 steps (with `enabled`).
    pub press_jitter: f32,
}

/// 字符轻重 steps: 更轻Ⅱ −2, 更轻Ⅰ −1, 标准 0, 更重Ⅰ +1, 更重Ⅱ +2. Random 轻重 may reach ±3.
pub const PRESS_MAX: f32 = 3.0;

impl HandwritingStyle {
    /// Pen pressure of the char with id `id` and 轻重 `tier`, in steps (0 = standard).
    pub fn press(&self, tier: i8, id: u64) -> f32 {
        let r = if self.enabled && self.press_jitter > 0.0 { (rng_for(self.seed, SALT_PRESS, id).unit() * 2.0 - 1.0) * self.press_jitter } else { 0.0 };
        (f32::from(tier) + r).clamp(-PRESS_MAX, PRESS_MAX)
    }
}

/// 连笔: inside a character, the pen stays down from one stroke's end to the next stroke's start
/// when they are close, drawing a short curve between them like a fast hand does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cursive {
    pub enabled: bool,
    /// 连笔距离: join only gaps up to this, percent of the character height.
    pub max_gap_pct: f32,
    /// 连笔概率: chance, percent, that a close pair is joined.
    pub chance_pct: f32,
}

impl Default for Cursive {
    fn default() -> Self {
        Self { enabled: false, max_gap_pct: 40.0, chance_pct: 60.0 }
    }
}

/// Points on a joining curve, end points excluded.
const JOIN_STEPS: usize = 6;

impl Cursive {
    /// Join one glyph's strokes (glyph units, y down; `em` units = one character height). Draws
    /// two values per stroke from `rng`, so a stroke's fate doesn't depend on the ones before it.
    pub fn join(&self, strokes: &[crate::layout::Stroke], em: f32, rng: &mut Rng) -> Vec<crate::layout::Stroke> {
        let max_gap = em * self.max_gap_pct / 100.0;
        let p = self.chance_pct / 100.0;
        let mut out: Vec<crate::layout::Stroke> = Vec::with_capacity(strokes.len());
        for s in strokes {
            let (draw, bulge) = (rng.unit(), rng.unit() * 2.0 - 1.0);
            if let (Some(prev), Some(&b)) = (out.last_mut(), s.first())
                && let Some(&a) = prev.last()
            {
                let gap = (b.0 - a.0).hypot(b.1 - a.1);
                if gap <= max_gap && draw < p {
                    // Leave a in the direction the stroke was going, bend a little to one side.
                    let dir = match prev.len().checked_sub(2).map(|i| prev[i]) {
                        Some(q) if (a.0 - q.0).hypot(a.1 - q.1) > 1e-6 => {
                            let l = (a.0 - q.0).hypot(a.1 - q.1);
                            ((a.0 - q.0) / l, (a.1 - q.1) / l)
                        }
                        _ => (0.0, 0.0),
                    };
                    if gap > 1e-6 {
                        let perp = (-(b.1 - a.1) / gap, (b.0 - a.0) / gap);
                        let c = (
                            (a.0 + b.0) / 2.0 + gap * (0.3 * dir.0 + 0.2 * bulge * perp.0),
                            (a.1 + b.1) / 2.0 + gap * (0.3 * dir.1 + 0.2 * bulge * perp.1),
                        );
                        for k in 1..=JOIN_STEPS {
                            let t = k as f32 / (JOIN_STEPS + 1) as f32;
                            let u = 1.0 - t;
                            prev.push((u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1));
                        }
                    }
                    prev.extend_from_slice(s);
                    continue;
                }
            }
            out.push(s.clone());
        }
        out
    }
}

/// Size, baseline, float and deformation of one script (English letters or digits), on top of
/// the Chinese settings. The defaults change nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptStyle {
    /// 大小, percent of the Chinese size (the advance scales too).
    pub scale_pct: f32,
    /// 基线偏移, mm; positive raises the glyph.
    pub baseline_mm: f32,
    /// 上下浮动; `None` = same as Chinese.
    pub vertical_offset: Option<Span>,
    /// 字符变形; `None` = same as Chinese.
    pub deformation: Option<crate::deformation::Deformation>,
}

impl Default for ScriptStyle {
    fn default() -> Self {
        Self { scale_pct: 100.0, baseline_mm: 0.0, vertical_offset: None, deformation: None }
    }
}

impl ScriptStyle {
    /// Size or baseline differ from Chinese.
    pub fn moves_glyph(&self) -> bool {
        self.scale_pct != 100.0 || self.baseline_mm != 0.0
    }
}

impl HandwritingStyle {
    /// The script settings that apply to `c`, if it is an English letter or a digit.
    pub fn script_of(&self, c: char) -> Option<&ScriptStyle> {
        use crate::font::fontset::{CharClass, class_of};
        match class_of(c) {
            CharClass::English => Some(&self.english),
            CharClass::Number => Some(&self.number),
            _ => None,
        }
    }
}

impl Default for HandwritingStyle {
    /// ChiXie3's shipped `[WriteConfig]` values, effects disabled.
    fn default() -> Self {
        Self {
            enabled: false,
            seed: 0,
            char_spacing_zh: Span::new(0.6, 1.1),
            char_spacing_en: Span::new(0.5, 1.0),
            space_width_zh: Span::new(5.0, 7.0),
            space_width_en: Span::new(2.0, 3.0),
            vertical_offset: Span::new(0.0, 0.3),
            row_offset_style: RowOffsetStyle::Independent,
            intelligent_vertical_offset: false,
            random_correct_offset: false,
            line_start_offset: Span::new(0.0, 2.0),
            first_line_indent: None,
            english: ScriptStyle::default(),
            number: ScriptStyle::default(),
            cursive: Cursive::default(),
            press_jitter: 0.0,
        }
    }
}

/// ChiXie3's default 首行缩进 range, mm (`min/max_first_line_indent`).
pub const DEFAULT_INDENT: Span = Span { min: 15.5, max: 17.0 };

impl HandwritingStyle {
    /// Width of a paragraph indent switched on by Tab or 首行缩进: drawn from the style's range
    /// (ChiXie3's 15.5–17 mm when unset), keyed by the paragraph's first char id.
    pub fn indent_width(&self, id: u64) -> f32 {
        self.first_line_indent.unwrap_or(DEFAULT_INDENT).sample(&mut rng_for(self.seed, SALT_INDENT, id))
    }
}

/// CJK ideographs, kana, hangul and full-width forms use the `_zh` ranges; everything else `_en`.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3134F)
}

/// Independent random stream for (`seed`, `salt`, `id`). Each character gets its own stream keyed
/// by its stable id, so inserting or deleting text never reshuffles the jitter of the other
/// characters (ChiXie3 bakes randomness per character the same way).
pub fn rng_for(seed: u64, salt: u64, id: u64) -> Rng {
    let mut r = Rng::new(seed ^ salt ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    Rng::new(r.next_u64())
}

const SALT_LINE: u64 = 0x4C49_4E45_0000_0001;
const SALT_GLYPH: u64 = 0x474C_5948_0000_0002;
const SALT_INDENT: u64 = 0x494E_4445_0000_0003;
/// Random row count per page (ruled templates).
pub const SALT_ROWS: u64 = 0x524F_5753_0000_0005;
/// 下划线 / 删除线 waver.
pub const SALT_RULE: u64 = 0x5255_4C45_0000_0008;
/// Deformation draws from its own stream so toggling jitter doesn't change deformation.
pub const SALT_DEFORM: u64 = 0x4445_464F_0000_0004;
/// 连笔 joins.
pub const SALT_CURSIVE: u64 = 0x4355_5253_0000_0009;
/// 随机轻重.
const SALT_PRESS: u64 = 0x5052_4553_0000_000A;
/// Inline table borders and flowchart outlines (keyed by the anchor char's id).
pub const SALT_FRAME: u64 = 0x4652_414D_0000_000B;

/// Jitter values for each character, keyed by the character's stable id.
pub struct Jitter<'a> {
    style: &'a HandwritingStyle,
    /// The current row's drift for `intelligent_vertical_offset`, −1…1 (set by `line_start`).
    row_sweep: f32,
}

/// Per-row 同向偏移 drift.
const SALT_ROW_DRIFT: u64 = 0x4452_4946_0000_0007;

impl<'a> Jitter<'a> {
    pub fn new(style: &'a HandwritingStyle) -> Self {
        Self { style, row_sweep: 0.0 }
    }

    fn rng(&self, salt: u64, id: u64) -> Rng {
        rng_for(self.style.seed, salt, id)
    }

    /// X offset of a line's first character; `id` = that character's id. Also starts the row's
    /// 同向偏移 drift.
    pub fn line_start(&mut self, id: u64) -> f32 {
        self.row_sweep = self.rng(SALT_ROW_DRIFT, id).unit() * 2.0 - 1.0;
        self.style.line_start_offset.sample(&mut self.rng(SALT_LINE, id))
    }

    /// Paragraph first-line indent, if configured.
    pub fn indent(&self, id: u64) -> Option<f32> {
        self.style.first_line_indent.map(|s| s.sample(&mut self.rng(SALT_INDENT, id)))
    }

    pub fn spacing(&self, c: char, id: u64) -> f32 {
        let s = &self.style;
        if is_cjk(c) { s.char_spacing_zh } else { s.char_spacing_en }.sample(&mut self.rng(SALT_GLYPH, id))
    }

    /// Space width; `prev` is the character before the space (decides zh vs en).
    pub fn space(&self, prev: Option<char>, id: u64) -> f32 {
        let s = &self.style;
        if prev.is_some_and(is_cjk) { s.space_width_zh } else { s.space_width_en }.sample(&mut self.rng(SALT_GLYPH, id))
    }

    /// Vertical offset (mm, down) of a glyph whose left edge is `x` mm along a row `row_w` mm
    /// wide. Independent per glyph, or with 智能偏移 (同向偏移) a drift in one direction along
    /// the row: each row draws a slope (keyed by its first character) so the offset runs from
    /// one side of the range to the other, plus a little noise.
    pub fn vertical(&mut self, id: u64, x: f32, row_w: f32) -> f32 {
        self.vertical_in(self.style.vertical_offset, id, x, row_w)
    }

    /// [`Self::vertical`] for character `c`: English letters and digits may have their own range.
    pub fn vertical_for(&mut self, c: char, id: u64, x: f32, row_w: f32) -> f32 {
        let r = self.style.script_of(c).and_then(|s| s.vertical_offset).unwrap_or(self.style.vertical_offset);
        self.vertical_in(r, id, x, row_w)
    }

    fn vertical_in(&mut self, r: Span, id: u64, x: f32, row_w: f32) -> f32 {
        // Second draw of the glyph stream (the first is spacing).
        let mut rng = self.rng(SALT_GLYPH, id);
        rng.next_u64();
        if !self.style.intelligent_vertical_offset {
            return r.sample(&mut rng);
        }
        let (mid, half) = ((r.min + r.max) / 2.0, (r.max - r.min) / 2.0);
        let w = if row_w.is_finite() && row_w > 1.0 { row_w } else { 150.0 };
        // Fraction of the range the row sweeps, −1…1 (halved by 随机纠正).
        let mut sweep = self.row_sweep;
        if self.style.random_correct_offset {
            sweep *= 0.5;
        }
        let along = (x / w).clamp(0.0, 1.0) - 0.5;
        let noise = (rng.unit() * 2.0 - 1.0) * half * 0.2;
        (mid + sweep * half * 2.0 * along * 0.9 + noise).clamp(r.min, r.max)
    }
}

/// On-disk style profile (`--style <toml>`, and later the GUI settings).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleFile {
    pub handwriting: HandwritingStyle,
    pub deformation: Deformation,
}

impl StyleFile {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| Error::Api(format!("style {}: {e}", path.display())))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("style serializes")
    }
}

/// CLI/GUI overrides on top of a style file, lowest to highest precedence:
/// built-in defaults (all off) < style file < `seed` < `no_jitter` / `deform`.
/// `deform` turns base deformation on with the style file's ranges (or ChiXie3 defaults).
pub fn resolve(file: Option<StyleFile>, seed: Option<u64>, no_jitter: bool, deform: bool) -> StyleFile {
    let mut s = file.unwrap_or_default();
    if let Some(seed) = seed {
        s.handwriting.seed = seed;
    }
    if no_jitter {
        s.handwriting.enabled = false;
    }
    if deform {
        s.deformation.enabled = true;
        if !s.deformation.base && !s.deformation.shape {
            s.deformation.base = true;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursive_joins_close_strokes_with_a_curve() {
        let a = vec![(0.0, 0.0), (3.0, 0.0)];
        let b = vec![(3.0, 1.0), (3.0, 4.0)];
        let far = vec![(0.0, 7.0), (3.0, 7.0)];
        let all = Cursive { enabled: true, max_gap_pct: 20.0, chance_pct: 100.0 };
        let out = all.join(&[a.clone(), b.clone(), far.clone()], 7.0, &mut Rng::new(1));
        // a + b are 1 apart (≤ 1.4): one stroke through a curve; `far` is 3 away: on its own.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), a.len() + JOIN_STEPS + b.len());
        assert_eq!((out[0][0], *out[0].last().unwrap()), (a[0], b[1]));
        assert_eq!(out[1], far);
        // The curve runs from a's end to b's start without jumps.
        assert!(out[0].windows(2).all(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) <= 3.0 + 1e-4));
        // Chance 0: nothing joins; stable per seed.
        let none = Cursive { chance_pct: 0.0, ..all.clone() };
        assert_eq!(none.join(&[a.clone(), b.clone()], 7.0, &mut Rng::new(1)), vec![a.clone(), b.clone()]);
        let half = Cursive { chance_pct: 50.0, ..all };
        assert_eq!(half.join(&[a.clone(), b.clone()], 7.0, &mut Rng::new(3)), half.join(&[a, b], 7.0, &mut Rng::new(3)));
    }

    #[test]
    fn press_is_the_tier_plus_bounded_jitter() {
        let off = HandwritingStyle { press_jitter: 1.0, ..Default::default() }; // enabled = false
        assert_eq!((off.press(0, 5), off.press(-2, 5), off.press(2, 5)), (0.0, -2.0, 2.0));
        let on = HandwritingStyle { enabled: true, seed: 3, press_jitter: 1.0, ..Default::default() };
        let v: Vec<f32> = (0..200).map(|id| on.press(1, id)).collect();
        assert!(v.iter().all(|p| (0.0..=2.0).contains(p)) && v.iter().any(|&p| p != 1.0));
        assert_eq!(on.press(1, 7), on.press(1, 7));
        // Never past ±PRESS_MAX.
        let wild = HandwritingStyle { press_jitter: 5.0, ..on };
        assert!((0..200).all(|id| wild.press(2, id).abs() <= PRESS_MAX));
    }

    #[test]
    fn rng_is_stable_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let xs: Vec<f32> = (0..1000).map(|_| a.unit()).collect();
        assert!(xs.iter().all(|x| (0.0..=1.0).contains(x)));
        assert_eq!(xs[..5], (0..5).map(|_| b.unit()).collect::<Vec<_>>()[..]);
        // Pinned values: the seed is persisted, so the stream must never change.
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn samples_stay_in_spans() {
        let style = HandwritingStyle { enabled: true, seed: 7, ..Default::default() };
        let mut j = Jitter::new(&style);
        for id in 0..500 {
            assert!(style.char_spacing_zh.contains(j.spacing('你', id)));
            assert!(style.char_spacing_en.contains(j.spacing('A', id)));
            assert!(style.space_width_zh.contains(j.space(Some('你'), id)));
            assert!(style.space_width_en.contains(j.space(Some('A'), id)));
            assert!(style.vertical_offset.contains(j.vertical(id, 0.0, 150.0)));
            assert!(style.line_start_offset.contains(j.line_start(id)));
        }
    }

    #[test]
    fn draws_depend_on_id_not_call_order() {
        let style = HandwritingStyle { enabled: true, seed: 7, ..Default::default() };
        let mut a = Jitter::new(&style);
        let mut b = Jitter::new(&style);
        // b draws for other ids first; a's values for id 42 must not care.
        for id in 0..10 {
            b.spacing('你', id);
            b.vertical(id, 0.0, 150.0);
        }
        assert_eq!(a.spacing('你', 42), b.spacing('你', 42));
        assert_eq!(a.vertical(42, 0.0, 150.0), b.vertical(42, 0.0, 150.0));
        assert_ne!(a.spacing('你', 42), a.spacing('你', 43));
    }

    #[test]
    fn same_direction_offset_drifts_along_each_row() {
        let style = HandwritingStyle { enabled: true, intelligent_vertical_offset: true, ..Default::default() };
        let mut j = Jitter::new(&style);
        let mut steep = 0;
        for row in 0..50u64 {
            j.line_start(row * 100);
            let v: Vec<f32> = (0..15).map(|k| j.vertical(row * 100 + k, k as f32 * 10.0, 150.0)).collect();
            assert!(v.iter().all(|&o| style.vertical_offset.contains(o)), "{v:?}");
            // Same direction: the row's end minus its start has the sign of the trend, with
            // the noise (±0.03 mm) far smaller than a clear drift.
            let (a, b) = (v[..3].iter().sum::<f32>() / 3.0, v[12..].iter().sum::<f32>() / 3.0);
            if (b - a).abs() > 0.1 {
                steep += 1;
                let dir = (b - a).signum();
                assert!(v.windows(5).all(|w| (w[4] - w[0]) * dir > -0.07), "{v:?}");
            }
        }
        assert!(steep > 10, "rows should drift visibly: {steep}");
        // The same row gives the same offsets.
        j.line_start(7);
        let first = j.vertical(8, 30.0, 150.0);
        j.line_start(7);
        assert_eq!(first, j.vertical(8, 30.0, 150.0));
    }

    #[test]
    fn precedence() {
        let file = StyleFile {
            handwriting: HandwritingStyle { enabled: true, seed: 1, ..Default::default() },
            ..Default::default()
        };
        // No file, no flags: everything off.
        let s = resolve(None, None, false, false);
        assert!(!s.handwriting.enabled && !s.deformation.enabled);
        // --seed overrides the file's seed; --no-jitter forces jitter off.
        let s = resolve(Some(file.clone()), Some(9), true, false);
        assert_eq!(s.handwriting.seed, 9);
        assert!(!s.handwriting.enabled);
        // --deform without a file turns base deformation on with default ranges.
        let s = resolve(None, None, false, true);
        assert!(s.deformation.enabled && s.deformation.base && !s.deformation.shape);
    }

    #[test]
    fn style_toml_roundtrips() {
        let s = StyleFile {
            handwriting: HandwritingStyle { enabled: true, seed: 3, ..Default::default() },
            ..Default::default()
        };
        let back: StyleFile = toml::from_str(&s.to_toml()).unwrap();
        assert_eq!(back, s);
        // Partial files fill the rest from defaults.
        let p: StyleFile = toml::from_str("[handwriting]\nenabled = true\n").unwrap();
        assert!(p.handwriting.enabled);
        assert_eq!(p.handwriting.char_spacing_zh, Span::new(0.6, 1.1));
    }
}
