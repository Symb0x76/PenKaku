//! ChiXie stroke fonts: decrypt the `get_font_access` payload and parse glyph strokes.
//!
//! Payload (see docs/server-api.md): `Fernet(key).decrypt(blob)` → JSON
//! `{ "<char>": [advance_width, stroke, stroke, ...] }` where
//! `stroke = [[x0,x1,...],[y0,y1,...]]`. Coordinates are a small float grid
//! (cell ~0..7 units, y increasing downward); pen is down along a stroke, up between strokes.


use std::collections::HashMap;

use crate::{Error, Result};

mod compose;
mod crypto;
pub use crypto::{fernet_decrypt, shape};

pub mod fix;
pub mod fontset;
pub mod library;
pub mod svg;
pub mod tags;

/// One point in glyph units (y downward).
pub type Point = (f32, f32);

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// Horizontal advance after this character, in glyph units.
    pub advance: f32,
    /// Each stroke is one pen-down polyline.
    pub strokes: Vec<Vec<Point>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Font {
    pub glyphs: HashMap<char, Glyph>,
}

impl Font {
    pub fn get(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c)
    }

    /// Candidate Fernet keys derived from the login `backup_token`. The server ships the
    /// 44-char urlsafe-b64 key with its `=` padding shifted off the end (obfuscation); the
    /// client restores it. Observed fix is swapping the last two chars, but the exact
    /// position can vary, so try a few normalizations and let HMAC pick the right one.
    pub fn key_candidates(backup_token: &str) -> Vec<String> {
        let mut out = Vec::new();
        let c: Vec<char> = backup_token.chars().collect();
        let n = c.len();
        if n >= 2 {
            let mut swapped = c.clone();
            swapped.swap(n - 1, n - 2);
            out.push(swapped.into_iter().collect());
        }
        // Move a misplaced '=' to the end (handles padding anywhere near the tail).
        if backup_token.contains('=') {
            out.push(format!("{}=", backup_token.replace('=', "")));
        }
        out.push(backup_token.to_string()); // raw, in case no obfuscation
        out
    }

    /// [`Self::compose_math`] and [`Self::compose_arrows`], for callers that do not need to know
    /// which symbols were made.
    pub fn with_composed_symbols(mut self) -> Self {
        self.compose_math();
        self.compose_arrows();
        self
    }

    /// Arrows the font lacks, drawn with its own strokes so text cleaned for handwriting
    /// (`->` → `→`) writes in the font's hand instead of a missing box: `→` is its `-` stretched
    /// to an arrow's length with its `>` on the right end, `⇒` uses `=`, `←` / `⇐` put `<` on the
    /// left, `↔` / `⇔` both. A one-stroke shaft runs on into the head without lifting the pen
    /// (连笔), as a hand writes an arrow. Arrows the font has, or whose parts it lacks, are left
    /// alone. Returns the arrows it made.
    pub fn compose_arrows(&mut self) -> Vec<char> {
        // Shaft length: at least this many times the head's width (a `-` alone is often short).
        const SHAFT_PER_HEAD: f32 = 2.0;
        // Room on each side of a stretched arrow, in head widths, so it does not touch its
        // neighbours.
        const SIDE_PER_HEAD: f32 = 0.25;
        const ARROWS: [(char, char, bool, bool); 6] =
            [('→', '-', false, true), ('←', '-', true, false), ('↔', '-', true, true), ('⇒', '=', false, true), ('⇐', '=', true, false), ('⇔', '=', true, true)];
        let mut made = Vec::new();
        for (arrow, shaft, left, right) in ARROWS {
            if self.glyphs.contains_key(&arrow) {
                continue;
            }
            let get = |c: char| crate::layout::glyph_for(self, c);
            let (Some(s), Some(lt), Some(gt)) = (get(shaft), get('<'), get('>')) else { continue };
            let Some((x0, sy0, x1, sy1)) = bounds(&s.strokes) else { continue };
            let head_w = [lt, gt].iter().filter_map(|g| bounds(&g.strokes)).map(|b| b.2 - b.0).fold(0.0, f32::max);
            // A short shaft is stretched to `len` and moved right by `side`; the arrow then runs
            // `a..b` and the advance grows by the stretch plus room on both sides.
            let len = (x1 - x0).max(head_w * SHAFT_PER_HEAD);
            let side = if len > x1 - x0 { head_w * SIDE_PER_HEAD } else { 0.0 };
            let k = if x1 > x0 { len / (x1 - x0) } else { 1.0 };
            let (a, b) = (x0 + side, x0 + side + len);
            let mid = (sy0 + sy1) / 2.0;
            let mut shafts: Vec<Vec<Point>> = s.strokes.iter().map(|st| st.iter().map(|&(x, y)| (a + (x - x0) * k, y)).collect()).collect();
            // 连笔 needs a single shaft stroke, drawn left to right.
            let joined = shafts.len() == 1;
            if joined && shafts[0].first().map(|p| p.0) > shafts[0].last().map(|p| p.0) {
                shafts[0].reverse();
            }
            let place = |g: &Glyph, at_left: bool| -> Vec<Vec<Point>> {
                let Some((hx0, hy0, hx1, hy1)) = bounds(&g.strokes) else { return Vec::new() };
                let dx = if at_left { a - hx0 } else { b - hx1 };
                let dy = mid - (hy0 + hy1) / 2.0;
                g.strokes.iter().map(|st| st.iter().map(|&(x, y)| (x + dx, y + dy)).collect()).collect()
            };
            let mut heads = Vec::new();
            if right {
                let h = place(gt, false);
                heads.extend(if joined { join_head(&mut shafts[0], h, false) } else { h });
            }
            if left {
                let h = place(lt, true);
                heads.extend(if joined { join_head(&mut shafts[0], h, true) } else { h });
            }
            let advance = s.advance + (len - (x1 - x0)) + 2.0 * side;
            shafts.extend(heads);
            self.glyphs.insert(arrow, Glyph { advance, strokes: shafts });
            made.push(arrow);
        }
        made
    }

    /// Back-compat: the first (most likely) key candidate.
    pub fn key_from_backup_token(backup_token: &str) -> String {
        Self::key_candidates(backup_token).into_iter().next().unwrap_or_default()
    }

    /// Decrypt a raw `get_font_access` payload with the derived key, then parse.
    pub fn from_encrypted(blob: &str, key: &str) -> Result<Self> {
        let plain = fernet_decrypt(blob, key)?;
        Self::from_json(&plain)
    }

    /// Decrypt by trying each key candidate derived from the backup_token.
    pub fn from_encrypted_with_backup_token(blob: &str, backup_token: &str) -> Result<Self> {
        let mut why = Vec::new();
        for (i, key) in Self::key_candidates(backup_token).into_iter().enumerate() {
            match fernet_decrypt(blob, &key) {
                Ok(plain) => {
                    log::debug!("font decrypt ok with key candidate {}", i + 1);
                    return Self::from_json(&plain);
                }
                Err(e) => why.push(format!("#{}: {e}", i + 1)),
            }
        }
        Err(Error::Font(format!("fernet decrypt failed [{}]; {}", why.join("; "), shape(backup_token, blob))))
    }

    /// The glyph map in the same JSON shape `from_json` reads (`{"字": [advance, [xs…], [ys…]]}`,
    /// one inner array per stroke), for the local font store.
    pub fn to_json(&self) -> Vec<u8> {
        let mut map = serde_json::Map::with_capacity(self.glyphs.len());
        for (c, g) in &self.glyphs {
            let xs: Vec<Vec<f32>> = g.strokes.iter().map(|s| s.iter().map(|p| p.0).collect()).collect();
            let ys: Vec<Vec<f32>> = g.strokes.iter().map(|s| s.iter().map(|p| p.1).collect()).collect();
            map.insert(c.to_string(), serde_json::json!([g.advance, xs, ys]));
        }
        serde_json::to_vec(&map).expect("glyph map serialises")
    }

    /// Parse the decrypted UTF-8 JSON glyph map.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        // Each value is [advance, stroke, stroke, ...]; a stroke is [xs, ys].
        // Use serde_json::Value so a malformed glyph can be skipped, not fail the whole font.
        let raw: HashMap<String, Vec<serde_json::Value>> =
            serde_json::from_slice(bytes).map_err(|e| Error::Font(format!("json: {e}")))?;
        let mut glyphs = HashMap::with_capacity(raw.len());
        for (k, parts) in raw {
            let Some(c) = one_char(&k) else { continue };
            let Some(glyph) = parse_glyph(&parts) else { continue };
            glyphs.insert(c, glyph);
        }
        Ok(Font { glyphs })
    }
}

/// Run the left-to-right `shaft` on into an arrow `head` (连笔): the head stroke through its tip
/// (its rightmost point for a right head, leftmost for a left one) is cut there, one arm is
/// drawn with the shaft in one pen-down stroke, and the other arm stays its own stroke. Returns
/// the head strokes still drawn apart.
fn join_head(shaft: &mut Vec<Point>, head: Vec<Vec<Point>>, at_left: bool) -> Vec<Vec<Point>> {
    let tip_of = |st: &Vec<Point>| -> Option<(usize, f32)> {
        st.iter().enumerate().map(|(i, p)| (i, p.0)).reduce(|m, c| if (c.1 > m.1) != at_left && c.1 != m.1 { c } else { m })
    };
    // The stroke holding the tip (the extreme x over the whole head).
    let Some((s, k)) = head
        .iter()
        .enumerate()
        .filter_map(|(s, st)| tip_of(st).map(|(k, x)| (s, k, x)))
        .reduce(|m, c| if (c.2 > m.2) != at_left && c.2 != m.2 { c } else { m })
        .map(|(s, k, _)| (s, k))
    else {
        return head;
    };
    let mut rest: Vec<Vec<Point>> = head.iter().enumerate().filter(|&(i, _)| i != s).map(|(_, st)| st.clone()).collect();
    let st = &head[s];
    let last = st.len() - 1;
    if at_left {
        // Arm into the tip, then the shaft: [.. tip] + shaft.
        let (lead, other): (Vec<Point>, Option<Vec<Point>>) =
            if k == 0 { (st.iter().rev().copied().collect(), None) } else { (st[..=k].to_vec(), (k < last).then(|| st[k..].to_vec())) };
        let mut joined = lead;
        let skip = usize::from(joined.last() == shaft.first());
        joined.extend(shaft.iter().skip(skip).copied());
        *shaft = joined;
        rest.extend(other);
    } else {
        // The shaft, then from the tip out along one arm: shaft + [tip ..].
        let (arm, other): (Vec<Point>, Option<Vec<Point>>) =
            if k == last { (st.iter().rev().copied().collect(), None) } else { (st[k..].to_vec(), (k > 0).then(|| st[..=k].to_vec())) };
        let skip = usize::from(shaft.last() == arm.first());
        shaft.extend(arm.into_iter().skip(skip));
        rest.extend(other);
    }
    rest
}

/// Bounding box `(x0, y0, x1, y1)` of every point of `strokes`, if there are any.
fn bounds(strokes: &[Vec<Point>]) -> Option<(f32, f32, f32, f32)> {
    let mut pts = strokes.iter().flatten();
    let &(x, y) = pts.next()?;
    Some(pts.fold((x, y, x, y), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y))))
}

fn one_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

/// Glyph JSON is `[advance, X, Y]`. X and Y are parallel coordinate containers:
/// either flat number arrays (one stroke) or arrays-of-arrays (one entry per stroke).
fn parse_glyph(parts: &[serde_json::Value]) -> Option<Glyph> {
    let advance = parts.first()?.as_f64()? as f32;
    let (Some(x), Some(y)) = (parts.get(1), parts.get(2)) else {
        return Some(Glyph { advance, strokes: vec![] });
    };
    let (xa, ya) = (x.as_array()?, y.as_array()?);
    let mut strokes = Vec::new();
    match xa.first() {
        // Flat: X = [x0, x1, ...]  -> a single polyline.
        Some(v) if v.is_number() => {
            if let Some(s) = zip_points(xa, ya) {
                strokes.push(s);
            }
        }
        // Nested: X = [[..],[..]], one inner array per stroke.
        Some(_) => {
            for (xi, yi) in xa.iter().zip(ya.iter()) {
                if let (Some(xs), Some(ys)) = (xi.as_array(), yi.as_array())
                    && let Some(s) = zip_points(xs, ys).filter(|p| !p.is_empty())
                {
                    strokes.push(s);
                }
            }
        }
        None => {}
    }
    Some(Glyph { advance, strokes })
}

/// Pair two coordinate arrays into points, truncating to the shorter.
fn zip_points(xs: &[serde_json::Value], ys: &[serde_json::Value]) -> Option<Vec<Point>> {
    let n = xs.len().min(ys.len());
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        pts.push((xs[i].as_f64()? as f32, ys[i].as_f64()? as f32));
    }
    Some(pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_swaps_last_two() {
        assert_eq!(Font::key_from_backup_token("abcd=i"), "abcdi=");
    }

    #[test]
    fn decrypts_python_fernet_token() {
        // Generated by pyca/cryptography Fernet; proves the Rust crate is cross-compatible.
        let key = "eYATO4_NrVMGe-hK54eMCMBPlyqXwQEOBAoQNQE7rY0=";
        let token = "gAAAAABq7Z1-Iyw8Zr0Cwd-oMu7e7s3t8acrzEafFSdLadD4XwXcKbiObWfMwzLOOSMMu8XxprTcoJzi8XcOIv_Wq_E3otmU2g==";
        let plain = fernet_decrypt(token, key).expect("decrypt python token");
        assert_eq!(String::from_utf8(plain).unwrap(), "héllo-stroke");
    }

    /// The server appends 2 junk chars to every font blob. With `=` padding the cut is after the
    /// `=`; fonts whose token needs no padding (花花体 #4, 巴拉体 #18, … failed before) get the junk
    /// right after the data. Token below: pyca Fernet, 32-byte plaintext → no padding.
    #[test]
    fn decrypts_unpadded_token_with_appended_junk() {
        let key = "eYATO4_NrVMGe-hK54eMCMBPlyqXwQEOBAoQNQE7rY0=";
        let token = "gAAAAABq7sq2qVgnB8zIOC-lX43xOf4dcNM-H4_9INUTAwlNeJFy40qw-pNtcWppcGEsahHWVxK62OuTk7KFU7-8XmXXXYsRmOhLblJuPfkSxWH3n2Ern75nA3nktI-KiwtyFs8oep9b";
        assert_eq!(token.len() % 4, 0);
        for junk in ["", "Qx", "x9"] {
            let plain = fernet_decrypt(&format!("{token}{junk}"), key).unwrap_or_else(|e| panic!("junk {junk:?}: {e}"));
            assert_eq!(plain, vec![b'x'; 32]);
        }
        // The padded python token with junk after its `=` run still works.
        let padded = "gAAAAABq7Z1-Iyw8Zr0Cwd-oMu7e7s3t8acrzEafFSdLadD4XwXcKbiObWfMwzLOOSMMu8XxprTcoJzi8XcOIv_Wq_E3otmU2g==ab";
        assert_eq!(String::from_utf8(fernet_decrypt(padded, key).unwrap()).unwrap(), "héllo-stroke");
    }

    #[test]
    fn parses_flat_and_nested_glyphs() {
        // I: flat single stroke (X,Y are number arrays). A: nested, two strokes.
        let json = br#"{
            "I": [1.0, [0.0, 0.0], [0.0, 7.0]],
            "A": [2.5, [[0.0,1.0],[2.0]], [[7.0,0.0],[3.0]]],
            "two": [1.0, [0.0], [0.0]]
        }"#;
        let f = Font::from_json(json).unwrap();
        assert_eq!(f.glyphs.len(), 2); // multi-char key "two" skipped
        let i = f.get('I').unwrap();
        assert_eq!(i.strokes.len(), 1);
        assert_eq!(i.strokes[0], vec![(0.0, 0.0), (0.0, 7.0)]);
        let a = f.get('A').unwrap();
        assert_eq!(a.advance, 2.5);
        assert_eq!(a.strokes.len(), 2);
        assert_eq!(a.strokes[0], vec![(0.0, 7.0), (1.0, 0.0)]);
        assert_eq!(a.strokes[1], vec![(2.0, 3.0)]);
    }

    #[test]
    fn missing_arrows_are_drawn_from_the_fonts_own_strokes() {
        let g = |advance: f32, strokes: Vec<Vec<Point>>| Glyph { advance, strokes };
        let mut font = Font {
            glyphs: HashMap::from([
                ('-', g(4.0, vec![vec![(0.0, 4.0), (3.0, 4.0)]])),
                ('>', g(2.0, vec![vec![(0.0, 2.0), (2.0, 3.0), (0.0, 4.0)]])),
                ('<', g(2.0, vec![vec![(2.0, 2.0), (0.0, 3.0), (2.0, 4.0)]])),
                ('⇒', g(9.0, vec![])),
            ]),
        };
        assert_eq!(font.compose_arrows(), vec!['→', '←', '↔']);
        // `→`: the 3-long shaft stretched to two head widths (4) and moved 0.5 right; `>` sits
        // with its tip on the shaft's right end, centred on it, and the shaft runs on into its
        // lower arm in one stroke (连笔). The advance grows by 1 + 2 × 0.5.
        let r = font.get('→').unwrap();
        assert_eq!(r.advance, 6.0);
        assert_eq!(r.strokes, vec![vec![(0.5, 4.0), (4.5, 4.0), (2.5, 5.0)], vec![(2.5, 3.0), (4.5, 4.0)]]);
        // `←`: the upper arm of `<` runs into the tip and on along the shaft.
        assert_eq!(font.get('←').unwrap().strokes, vec![vec![(2.5, 3.0), (0.5, 4.0), (4.5, 4.0)], vec![(0.5, 4.0), (2.5, 5.0)]]);
        // `↔`: one stroke from the left arm through the shaft into the right arm, plus two arms.
        assert_eq!(font.get('↔').unwrap().strokes.len(), 3);
        // The font's own `⇒` stays; `=` is missing, so no `⇐` / `⇔`.
        assert_eq!(font.get('⇒').unwrap().advance, 9.0);
        assert!(font.get('⇐').is_none() && font.get('⇔').is_none());
    }
}
