//! SVG fonts: the on-disk format of the font library. One file per font, one `<glyph>` per
//! character with open single-line paths in stroke order, so a file can be copied to share a font
//! and Inkscape's Hershey Text extension can use it too.
//!
//! Glyph units stay ChiXie's (≈0..7, y down); SVG fonts are y up from the baseline, so a glyph
//! point (x, y) is written as (x, 7 − y). The subtraction is done in f64, which is exact for any
//! f32 glyph coordinate, and every number is printed in its shortest round-trip form: reading a
//! file we wrote gives back the very same `f32`s, so handwriting stays byte-identical.
//! Foreign SVG fonts (other `units-per-em` / `ascent`) are scaled into the same units.

use std::fmt::Write as _;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{Font, Glyph};
use crate::{Error, Result};

/// Units per em of the files we write: ChiXie glyphs span about 0..7.
const EM: f64 = 7.0;
const NS: &str = "urn:penkaku:font:1";

/// Where a font came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSource {
    /// Downloaded from the ChiXie cloud (font id).
    Cloud { id: i64 },
    /// Made by penkaku (e.g. recorded or converted).
    Generated,
    /// Any other SVG font.
    Imported,
}

/// What a font file says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMeta {
    pub name: String,
    pub source: FontSource,
}

/// Serialise `font` as an SVG font. Glyphs are sorted by character, so the same font always
/// gives the same bytes.
pub fn write(font: &Font, meta: &FontMeta) -> String {
    let (source, cloud) = match meta.source {
        FontSource::Cloud { id } => ("cloud", format!(r#" cloud-id="{id}""#)),
        FontSource::Generated => ("generated", String::new()),
        FontSource::Imported => ("imported", String::new()),
    };
    let name = quick_xml::escape::escape(meta.name.as_str());
    let mut s = String::new();
    s += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\">\n";
    let _ = writeln!(s, r#"<metadata><penkaku:font xmlns:penkaku="{NS}" name="{name}" source="{source}"{cloud} chars="{}"/></metadata>"#, font.glyphs.len());
    let _ = writeln!(s, r#"<defs><font id="penkaku-font" horiz-adv-x="{EM}">"#);
    let _ = writeln!(s, r#"<font-face font-family="{name}" units-per-em="{EM}" ascent="{EM}" descent="0"/>"#);
    let mut chars: Vec<&char> = font.glyphs.keys().collect();
    chars.sort_unstable();
    for c in chars {
        let g = &font.glyphs[c];
        let mut d = String::new();
        for stroke in &g.strokes {
            for (k, &(x, y)) in stroke.iter().enumerate() {
                let _ = write!(d, "{}{} {} ", if k == 0 { "M" } else { "L" }, x, EM - f64::from(y));
            }
        }
        let _ = writeln!(s, r#"<glyph unicode="{}" horiz-adv-x="{}" d="{}"/>"#, unicode_attr(*c), g.advance, d.trim_end());
    }
    s += "</font></defs>\n</svg>\n";
    s
}

/// A char as an attribute value: whitespace and control chars as character references, so
/// attribute normalisation cannot turn them into plain spaces.
fn unicode_attr(c: char) -> String {
    if c.is_whitespace() || c.is_control() { format!("&#x{:X};", c as u32) } else { quick_xml::escape::escape(c.to_string()).into_owned() }
}

/// Parse an SVG font. Glyphs whose `unicode` is not exactly one char, or whose path uses
/// anything but straight lines, are skipped (like the JSON reader skips malformed glyphs).
pub fn read(bytes: &[u8]) -> Result<(Font, FontMeta)> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Font(format!("svg: {e}")))?;
    let mut r = Reader::from_str(text);
    let mut font = Font::default();
    let (mut name, mut family, mut source) = (None, None, FontSource::Imported);
    let (mut upm, mut ascent, mut default_adv) = (1000.0f64, None, 0.0f64);
    let mut glyphs: Vec<(String, Option<f64>, String)> = Vec::new();
    loop {
        let ev = r.read_event().map_err(|e| Error::Font(format!("svg: {e}")))?;
        let e = match &ev {
            Event::Start(e) | Event::Empty(e) => e,
            Event::Eof => break,
            _ => continue,
        };
        match e.local_name().as_ref() {
            b"font" if e.name().as_ref() == b"penkaku:font" => {
                name = attr(e, "name");
                source = match (attr(e, "source").as_deref(), attr(e, "cloud-id").and_then(|v| v.parse().ok())) {
                    (Some("cloud"), Some(id)) => FontSource::Cloud { id },
                    (Some("generated"), _) => FontSource::Generated,
                    _ => FontSource::Imported,
                };
            }
            b"font" => default_adv = num(e, "horiz-adv-x").unwrap_or(0.0),
            b"font-face" => {
                family = attr(e, "font-family");
                upm = num(e, "units-per-em").filter(|v| *v > 0.0).unwrap_or(1000.0);
                ascent = num(e, "ascent");
            }
            b"glyph" => {
                if let (Some(u), Some(d)) = (attr(e, "unicode"), attr(e, "d").or_else(|| Some(String::new()))) {
                    glyphs.push((u, num(e, "horiz-adv-x"), d));
                }
            }
            _ => {}
        }
    }
    let ascent = ascent.unwrap_or(upm * 0.8);
    let k = EM / upm;
    for (u, adv, d) in glyphs {
        let mut it = u.chars();
        let (Some(c), None) = (it.next(), it.next()) else { continue };
        let Some(strokes) = parse_path(&d, |x, y| ((x * k) as f32, ((ascent - y) * k) as f32)) else { continue };
        font.glyphs.insert(c, Glyph { advance: (adv.unwrap_or(default_adv) * k) as f32, strokes });
    }
    let name = name.or(family).unwrap_or_default();
    Ok((font, FontMeta { name, source }))
}

/// Name, source and character count without parsing the glyphs (for library scans): reads up to
/// the first `<glyph>`. `None` when the bytes are not an SVG font.
pub fn read_meta(bytes: &[u8]) -> Option<(FontMeta, usize)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut r = Reader::from_str(text);
    let (mut name, mut family, mut source, mut chars, mut is_font) = (None, None, FontSource::Imported, None, false);
    loop {
        let ev = r.read_event().ok()?;
        let e = match &ev {
            Event::Start(e) | Event::Empty(e) => e,
            Event::Eof => break,
            _ => continue,
        };
        match e.local_name().as_ref() {
            b"font" if e.name().as_ref() == b"penkaku:font" => {
                name = attr(e, "name");
                chars = attr(e, "chars").and_then(|v| v.parse().ok());
                source = match (attr(e, "source").as_deref(), attr(e, "cloud-id").and_then(|v| v.parse().ok())) {
                    (Some("cloud"), Some(id)) => FontSource::Cloud { id },
                    (Some("generated"), _) => FontSource::Generated,
                    _ => FontSource::Imported,
                };
            }
            b"font" => is_font = true,
            b"font-face" => family = attr(e, "font-family"),
            b"glyph" => break,
            _ => {}
        }
    }
    if !is_font {
        return None;
    }
    let chars = chars.unwrap_or_else(|| text.matches("<glyph ").count());
    Some((FontMeta { name: name.or(family).unwrap_or_default(), source }, chars))
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    let a = e.try_get_attribute(name).ok()??;
    Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned())
}

fn num(e: &BytesStart, name: &str) -> Option<f64> {
    attr(e, name)?.trim().parse().ok()
}

/// Strokes of a path made of absolute `M` / `L` commands (coordinates after `M` without a
/// command letter are line-tos, as in SVG). `None` for anything else.
fn parse_path(d: &str, map: impl Fn(f64, f64) -> (f32, f32)) -> Option<Vec<Vec<(f32, f32)>>> {
    let mut strokes: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let mut cmd = None;
    let flush = |cmd: Option<char>, nums: &mut Vec<f64>, strokes: &mut Vec<Vec<(f32, f32)>>| -> Option<()> {
        if !nums.len().is_multiple_of(2) {
            return None;
        }
        for (k, p) in nums.chunks(2).enumerate() {
            let pt = map(p[0], p[1]);
            match (cmd?, k) {
                ('M', 0) => strokes.push(vec![pt]),
                ('M' | 'L', _) => strokes.last_mut()?.push(pt),
                _ => return None,
            }
        }
        nums.clear();
        Some(())
    };
    let mut token = String::new();
    for ch in d.chars().chain(std::iter::once(' ')) {
        match ch {
            'M' | 'L' => {
                push_num(&mut token, &mut nums)?;
                flush(cmd, &mut nums, &mut strokes)?;
                cmd = Some(ch);
            }
            c if c.is_ascii_alphabetic() && c != 'e' && c != 'E' => return None,
            ' ' | ',' | '\t' | '\n' | '\r' => push_num(&mut token, &mut nums)?,
            '-' if !token.is_empty() && !token.ends_with(['e', 'E']) => {
                push_num(&mut token, &mut nums)?;
                token.push('-');
            }
            c => token.push(c),
        }
    }
    flush(cmd, &mut nums, &mut strokes)?;
    strokes.retain(|s| !s.is_empty());
    Some(strokes)
}

fn push_num(token: &mut String, nums: &mut Vec<f64>) -> Option<()> {
    if !token.is_empty() {
        nums.push(token.parse().ok()?);
        token.clear();
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/font_fixture.json");

    fn meta() -> FontMeta {
        FontMeta { name: "演示 <体> & \"x\"".into(), source: FontSource::Cloud { id: 42 } }
    }

    #[test]
    fn our_files_round_trip_exactly() {
        let mut f = Font::from_json(FIXTURE).unwrap();
        // Awkward values: tiny, negative, many digits; and chars that need escaping.
        f.glyphs.insert('<', Glyph { advance: 1.234_567_8, strokes: vec![vec![(0.1, 6.9), (-0.333_333_34, 0.000_123_4)]] });
        f.glyphs.insert(' ', Glyph { advance: 3.5, strokes: vec![] });
        f.glyphs.insert('\u{3000}', Glyph { advance: 7.0, strokes: vec![] });
        let svg = write(&f, &meta());
        assert_eq!(svg, write(&f, &meta()), "deterministic");
        let (back, m) = read(svg.as_bytes()).unwrap();
        assert_eq!(m, meta());
        assert_eq!(back, f);
    }

    #[test]
    fn reads_a_foreign_hershey_style_font() {
        // units-per-em 1000, ascent 800, y up; 'A' advance 600 with two strokes, the second
        // given as implicit line-tos after M with commas and a leading minus.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><font horiz-adv-x="500">
            <font-face font-family="Hershey Sans" units-per-em="1000" ascent="800" descent="-200"/>
            <glyph unicode="A" horiz-adv-x="600" d="M 0 0 L 300 800 L 600 0 M100,300 500,300"/>
            <glyph unicode="B" d="M0 0 C 1 1 2 2 3 3"/>
            <glyph unicode="ab" d="M0 0 L 1 1"/>
            <glyph unicode="-" d="M-100-0.5L100 0"/>
        </font></defs></svg>"#;
        let (f, m) = read(svg.as_bytes()).unwrap();
        assert_eq!((m.name.as_str(), m.source), ("Hershey Sans", FontSource::Imported));
        assert_eq!(f.glyphs.len(), 2, "curves and multi-char glyphs are skipped");
        let a = f.get('A').unwrap();
        assert!((a.advance - 4.2).abs() < 1e-6);
        assert_eq!(a.strokes.len(), 2);
        // (0, 0) baseline → y down from the top: (0, 800·0.007) = (0, 5.6); apex (300, 800) → (2.1, 0).
        assert!((a.strokes[0][0].1 - 5.6).abs() < 1e-6 && (a.strokes[0][1].0 - 2.1).abs() < 1e-6 && a.strokes[0][1].1.abs() < 1e-6);
        assert_eq!(a.strokes[1].len(), 2);
        let dash = f.get('-').unwrap();
        assert!((dash.strokes[0][0].0 + 0.7).abs() < 1e-6, "{:?}", dash.strokes);
        // No advance on the glyph: the font's default.
        assert!((dash.advance - 3.5).abs() < 1e-6);
    }
}
