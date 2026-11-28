//! `--marks`: light local formatting for plain-text `write` / `pdf`, without counting character
//! offsets. A rule picks paragraphs by number (`line`) or text (`match`) and says how to format
//! them (`f`):
//!
//! ```json
//! [{"line": 1, "f": {"align": "center", "size": 1.3}},
//!  {"line": [2, -2], "f": {"indent": 2}},
//!  {"match": "此致", "nth": 1, "f": {"indent": 0}},
//!  {"line": -1, "f": {"align": "right"}}]
//! ```
//!
//! TOML takes the same rules as `[[mark]]` tables. Paragraphs are `'\n'`-separated and counted
//! from 1; negative numbers count from the end (`-1` = the last). `line` is one number or an
//! inclusive `[first, last]` range. `match` formats every occurrence (or only the `nth`, from
//! 1): its characters take the character formats, its paragraph the paragraph formats. Later
//! rules win.

use std::ops::Range;
use std::path::Path;

use penkaku_core::layout::{Align, CharStyle, Para};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    #[serde(default)]
    pub line: Option<Lines>,
    #[serde(default, rename = "match")]
    pub text: Option<String>,
    #[serde(default)]
    pub nth: Option<usize>,
    pub f: Format,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Lines {
    One(i64),
    Range([i64; 2]),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    /// Paragraph: left / center / right / justify.
    pub align: Option<Align>,
    /// Paragraph: first-line indent in characters (0 = none).
    pub indent: Option<f32>,
    /// Characters: size factor (1.0 = the page's size).
    pub size: Option<f32>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Characters: −1 left, 0 upright, 1 right.
    pub tilt: Option<i8>,
}

#[derive(Deserialize)]
struct TomlFile {
    mark: Vec<Mark>,
}

/// Read a `.json` (an array of rules) or `.toml` (`[[mark]]` tables) marks file.
pub fn load(path: &Path) -> Result<Vec<Mark>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let is_toml = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml"));
    let parsed = if is_toml { toml::from_str::<TomlFile>(&text).map(|f| f.mark).map_err(|e| e.to_string()) } else { serde_json::from_str(&text).map_err(|e| e.to_string()) };
    parsed.map_err(|e| format!("marks {}: {e}", path.display()))
}

/// Paragraph formats (one per paragraph) and character formats (one per char) for `chars`;
/// `em_mm` turns an indent in characters into mm.
pub fn apply(marks: &[Mark], chars: &[char], em_mm: f32) -> (Vec<Para>, Vec<Option<CharStyle>>) {
    // Char range of each paragraph (its '\n' excluded).
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            spans.push(start..i);
            start = i + 1;
        }
    }
    spans.push(start..chars.len());
    let mut paras = vec![Para::default(); spans.len()];
    let mut styles: Vec<Option<CharStyle>> = vec![None; chars.len()];
    let para_of = |i: usize| spans.iter().position(|s| i <= s.end).unwrap_or(spans.len() - 1);
    for m in marks {
        let (ps, cs): (Vec<usize>, Vec<Range<usize>>) = match (&m.line, &m.text) {
            (Some(l), _) => {
                let ps = pick_lines(l, spans.len());
                let cs = ps.iter().map(|&p| spans[p].clone()).collect();
                (ps, cs)
            }
            (None, Some(t)) => {
                let hits = find(chars, t);
                let hits: Vec<Range<usize>> = match m.nth {
                    Some(n) => hits.into_iter().nth(n.saturating_sub(1)).into_iter().collect(),
                    None => hits,
                };
                (hits.iter().map(|r| para_of(r.start)).collect(), hits)
            }
            (None, None) => continue,
        };
        let f = &m.f;
        for p in ps {
            if let Some(a) = f.align {
                paras[p].align = a;
            }
            if let Some(n) = f.indent {
                paras[p].first_indent = Some(n.max(0.0) * em_mm);
            }
        }
        for i in cs.into_iter().flatten() {
            let mut s = styles[i].unwrap_or_default();
            s.scale = f.size.unwrap_or(s.scale);
            s.underline = f.underline.unwrap_or(s.underline);
            s.strike = f.strike.unwrap_or(s.strike);
            s.tilt = f.tilt.map_or(s.tilt, |t| t.clamp(-1, 1));
            styles[i] = (!s.is_plain()).then_some(s);
        }
    }
    (paras, styles)
}

/// Paragraph indices (0-based) a `line` picks out of `n`; out-of-range numbers pick nothing.
fn pick_lines(l: &Lines, n: usize) -> Vec<usize> {
    let ix = |k: i64| -> Option<usize> {
        let i = if k < 0 { n as i64 + k } else { k - 1 };
        (0..n as i64).contains(&i).then_some(i as usize)
    };
    match *l {
        Lines::One(k) => ix(k).into_iter().collect(),
        Lines::Range([a, b]) => match (ix(a), ix(b)) {
            (Some(a), Some(b)) => (a.min(b)..=a.max(b)).collect(),
            _ => Vec::new(),
        },
    }
}

/// Every occurrence of `t` in `chars`, left to right, never overlapping.
fn find(chars: &[char], t: &str) -> Vec<Range<usize>> {
    let pat: Vec<char> = t.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while !pat.is_empty() && i + pat.len() <= chars.len() {
        if chars[i..i + pat.len()] == pat[..] {
            out.push(i..i + pat.len());
            i += pat.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(json: &str) -> Vec<Mark> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn line_and_match_rules_map_to_paragraph_and_char_formats() {
        let text: Vec<char> = "标题\n第一段\n第二段此致\n敬礼\n张三".chars().collect();
        let m = marks(
            r#"[{"line": 1, "f": {"align": "center", "size": 1.5}},
                {"line": [2, -2], "f": {"indent": 2}},
                {"match": "此致", "nth": 1, "f": {"indent": 0, "underline": true}},
                {"line": -1, "f": {"align": "right"}},
                {"line": 9, "f": {"align": "right"}}]"#,
        );
        let (paras, styles) = apply(&m, &text, 5.0);
        let al: Vec<Align> = paras.iter().map(|p| p.align).collect();
        assert_eq!(al, [Align::Center, Align::Left, Align::Left, Align::Left, Align::Right]);
        let ind: Vec<Option<f32>> = paras.iter().map(|p| p.first_indent).collect();
        // The 此致 rule comes later and wins for its paragraph.
        assert_eq!(ind, [None, Some(10.0), Some(0.0), Some(10.0), None]);
        assert!(styles[..2].iter().all(|s| s.is_some_and(|s| s.scale == 1.5)) && styles[2].is_none());
        assert!(styles[10..12].iter().all(|s| s.is_some_and(|s| s.underline)) && styles[9].is_none());
    }

    #[test]
    fn toml_and_json_files_load() {
        let dir = std::env::temp_dir().join(format!("penkaku-marks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = dir.join("m.toml");
        std::fs::write(&t, "[[mark]]\nline = -1\nf = { align = \"right\" }\n\n[[mark]]\nmatch = \"此致\"\nf = { indent = 0 }\n").unwrap();
        let m = load(&t).unwrap();
        assert_eq!(m.len(), 2);
        assert!(matches!(m[0].line, Some(Lines::One(-1))) && m[1].text.as_deref() == Some("此致"));
        let j = dir.join("m.json");
        std::fs::write(&j, r#"[{"line": [1, 2], "f": {"bogus": 1}}]"#).unwrap();
        assert!(load(&j).unwrap_err().contains("bogus"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
