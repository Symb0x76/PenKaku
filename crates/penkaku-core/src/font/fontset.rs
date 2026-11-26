//! Several fonts working as one: role fonts per character class (主线 / 英文 / 符号 / 数字), a
//! fallback chain for characters a font lacks, and weighted mixing (字体混用) keyed by each
//! character's stable id, so the same character in the document keeps its font across edits while
//! repeated characters still vary like real handwriting.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::library::FontKey;
use super::{Font, Glyph};
use crate::handwriting::rng_for;
use crate::layout::kinsoku;

/// Mixing draws from its own random stream.
const SALT_FONT: u64 = 0x464F_4E54_0000_0006;

/// What layout needs from fonts: a glyph for a character (with its stable id, for mixing) and
/// the space glyph.
pub trait Glyphs {
    fn glyph(&self, c: char, id: u64) -> Option<&Glyph>;
    /// The main font's own `' '` glyph, if any (its advance is the space width).
    fn space(&self) -> Option<&Glyph>;
}

impl Glyphs for Font {
    fn glyph(&self, c: char, _id: u64) -> Option<&Glyph> {
        crate::layout::glyph_for(self, c)
    }
    fn space(&self) -> Option<&Glyph> {
        self.get(' ')
    }
}

/// The four role fonts (设为主线 / 英文 / 符号 / 数字字体). A role without a font uses `main`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontRoles {
    pub main: Option<FontKey>,
    pub english: Option<FontKey>,
    pub symbol: Option<FontKey>,
    pub number: Option<FontKey>,
}

impl FontRoles {
    /// Every font a role names, main first, without repeats.
    pub fn keys(&self) -> Vec<FontKey> {
        let mut v: Vec<FontKey> = Vec::new();
        for k in [&self.main, &self.english, &self.symbol, &self.number].into_iter().flatten() {
            if !v.contains(k) {
                v.push(k.clone());
            }
        }
        v
    }
}

/// One font in the 字体混用 mix and its weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixEntry {
    pub key: FontKey,
    pub weight: f32,
}

/// Which role font a character asks for first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Chinese,
    English,
    Number,
    Symbol,
}

pub fn class_of(c: char) -> CharClass {
    let u = c as u32;
    if c.is_ascii_digit() || (0xFF10..=0xFF19).contains(&u) {
        CharClass::Number
    } else if c.is_ascii_alphabetic() || (0xFF21..=0xFF3A).contains(&u) || (0xFF41..=0xFF5A).contains(&u) {
        CharClass::English
    } else if c.is_ascii_punctuation()
        || kinsoku::is_closing(c)
        || kinsoku::is_opening(c)
        || (0x2000..=0x206F).contains(&u)
        || (0x3000..=0x303F).contains(&u)
        || (0xFF01..=0xFF65).contains(&u)
    {
        CharClass::Symbol
    } else {
        CharClass::Chinese
    }
}

/// Loaded fonts plus the order to try them in for each character class.
#[derive(Debug, Clone, Default)]
pub struct FontSet {
    fonts: Vec<Arc<Font>>,
    /// The library key of each font (empty for [`FontSet::single`]).
    keys: Vec<FontKey>,
    /// Font indices to try, per class (Chinese, English, Number, Symbol).
    chains: [Vec<usize>; 4],
    /// 字体混用 for Chinese characters: (font index, weight); empty = off.
    mix: Vec<(usize, f32)>,
    seed: u64,
}

fn slot(c: CharClass) -> usize {
    match c {
        CharClass::Chinese => 0,
        CharClass::English => 1,
        CharClass::Number => 2,
        CharClass::Symbol => 3,
    }
}

impl FontSet {
    /// One font for everything (what layout used before font sets).
    pub fn single(font: Arc<Font>) -> Self {
        Self { fonts: vec![font], keys: Vec::new(), chains: std::array::from_fn(|_| vec![0]), mix: Vec::new(), seed: 0 }
    }

    /// Build from the loaded fonts. Each class tries its role font, then the main font, then the
    /// mixed fonts, then every other loaded font, in `loaded` order. Keys that are not loaded
    /// are ignored. `seed` keys the mixing (the document seed plus 换一组's salt).
    pub fn build(loaded: &[(FontKey, Arc<Font>)], roles: &FontRoles, mix: &[MixEntry], seed: u64) -> Self {
        let ix = |k: &Option<FontKey>| k.as_ref().and_then(|k| loaded.iter().position(|(l, _)| l == k));
        let mixed: Vec<(usize, f32)> = mix.iter().filter(|m| m.weight > 0.0).filter_map(|m| Some((ix(&Some(m.key.clone()))?, m.weight))).collect();
        let main = ix(&roles.main).or(if loaded.is_empty() { None } else { Some(0) });
        let chain = |role: Option<usize>| {
            let mut v = Vec::new();
            for i in role.into_iter().chain(main).chain(mixed.iter().map(|m| m.0)).chain(0..loaded.len()) {
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        };
        let chains = [chain(None), chain(ix(&roles.english)), chain(ix(&roles.number)), chain(ix(&roles.symbol))];
        Self { fonts: loaded.iter().map(|(_, f)| f.clone()).collect(), keys: loaded.iter().map(|(k, _)| k.clone()).collect(), chains, mix: mixed, seed }
    }

    /// The library key of the font that draws `c` (with stable id `id`).
    pub fn key_of(&self, c: char, id: u64) -> Option<&FontKey> {
        self.pick(c, id).and_then(|(i, _)| self.keys.get(i))
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// The font index and glyph used for `c` (with stable id `id`); `None` when no font has it.
    pub fn pick(&self, c: char, id: u64) -> Option<(usize, &Glyph)> {
        let class = class_of(c);
        let first = (class == CharClass::Chinese && !self.mix.is_empty()).then(|| self.mixed_font(id));
        first.into_iter().chain(self.chains[slot(class)].iter().copied()).find_map(|i| Some((i, crate::layout::glyph_for(&self.fonts[i], c)?)))
    }

    /// Weighted draw keyed by the character id: stable for a character, independent across
    /// characters.
    fn mixed_font(&self, id: u64) -> usize {
        let total: f32 = self.mix.iter().map(|m| m.1).sum();
        let mut r = rng_for(self.seed, SALT_FONT, id).unit() * total;
        for &(i, w) in &self.mix {
            if r < w {
                return i;
            }
            r -= w;
        }
        self.mix.last().map_or(0, |m| m.0)
    }
}

impl Glyphs for FontSet {
    fn glyph(&self, c: char, id: u64) -> Option<&Glyph> {
        self.pick(c, id).map(|(_, g)| g)
    }
    fn space(&self) -> Option<&Glyph> {
        self.chains[0].first().and_then(|&i| self.fonts[i].get(' '))
    }
}

/// Per-character fonts (a run's 字体 in the editor) over the document's fonts: a char whose id
/// has an override takes that font's glyph, else (or when that font lacks it) `base`'s.
pub struct RunFonts<'a, G: Glyphs + ?Sized> {
    pub base: &'a G,
    pub by_id: std::collections::HashMap<u64, Arc<Font>>,
}

impl<G: Glyphs + ?Sized> Glyphs for RunFonts<'_, G> {
    fn glyph(&self, c: char, id: u64) -> Option<&Glyph> {
        self.by_id.get(&id).and_then(|f| crate::layout::glyph_for(f, c)).or_else(|| self.base.glyph(c, id))
    }
    fn space(&self) -> Option<&Glyph> {
        self.base.space()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(chars: &str) -> Arc<Font> {
        Arc::new(Font { glyphs: chars.chars().map(|c| (c, Glyph { advance: 7.0, strokes: vec![] })).collect() })
    }

    fn key(s: &str) -> FontKey {
        FontKey(s.into())
    }

    #[test]
    fn classes() {
        assert_eq!([class_of('7'), class_of('７'), class_of('a'), class_of('Ｚ')], [CharClass::Number, CharClass::Number, CharClass::English, CharClass::English]);
        assert_eq!([class_of('，'), class_of('.'), class_of('《'), class_of('你')], [CharClass::Symbol, CharClass::Symbol, CharClass::Symbol, CharClass::Chinese]);
    }

    #[test]
    fn roles_route_classes_and_missing_chars_fall_through() {
        let loaded = vec![(key("main"), font("你好a1，")), (key("en"), font("ab")), (key("num"), font("12")), (key("other"), font("龘"))];
        let roles = FontRoles { main: Some(key("main")), english: Some(key("en")), number: Some(key("num")), symbol: None };
        let s = FontSet::build(&loaded, &roles, &[], 0);
        assert_eq!(s.pick('a', 0).unwrap().0, 1, "english role");
        assert_eq!(s.pick('1', 0).unwrap().0, 2, "number role");
        assert_eq!(s.pick('，', 0).unwrap().0, 0, "no symbol role: main");
        assert_eq!(s.pick('你', 0).unwrap().0, 0);
        // Missing in the role font: main; missing everywhere but one: that one.
        assert_eq!(s.pick('3', 0), None);
        assert_eq!(s.pick('龘', 0).unwrap().0, 3);
        // Width counterpart is tried per font: '，' for ','.
        assert_eq!(s.pick(',', 0).unwrap().0, 0);
        // 字符微调 finds the font by key.
        assert_eq!(s.key_of('a', 0), Some(&key("en")));
        assert_eq!(s.key_of('3', 0), None);
        assert_eq!(FontSet::single(font("a")).key_of('a', 0), None);
    }

    #[test]
    fn mixing_3_to_1_is_within_3_percent_over_1000_chars() {
        let both = "你";
        let loaded = vec![(key("a"), font(both)), (key("b"), font(both))];
        let mix = [MixEntry { key: key("a"), weight: 3.0 }, MixEntry { key: key("b"), weight: 1.0 }];
        let s = FontSet::build(&loaded, &FontRoles { main: Some(key("a")), ..Default::default() }, &mix, 1234);
        let a = (0..1000u64).filter(|&id| s.pick('你', id).unwrap().0 == 0).count();
        assert!((720..=780).contains(&a), "{a} of 1000 from the weight-3 font");
        // Stable per id, and a new seed re-rolls.
        assert_eq!(s.pick('你', 17), s.pick('你', 17));
        let other = FontSet::build(&loaded, &FontRoles { main: Some(key("a")), ..Default::default() }, &mix, 99);
        assert!((0..100u64).any(|id| s.pick('你', id).unwrap().0 != other.pick('你', id).unwrap().0));
    }

    #[test]
    fn a_mixed_font_without_the_glyph_falls_back() {
        let loaded = vec![(key("a"), font("你好")), (key("b"), font("你"))];
        let mix = [MixEntry { key: key("b"), weight: 1.0 }];
        let s = FontSet::build(&loaded, &FontRoles::default(), &mix, 0);
        assert_eq!(s.pick('你', 3).unwrap().0, 1);
        assert_eq!(s.pick('好', 3).unwrap().0, 0);
    }
}
