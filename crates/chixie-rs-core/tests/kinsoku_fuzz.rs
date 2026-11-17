//! 避头尾: a wrapped line never starts with a closing mark or ends with an opening mark, on
//! free and grid pages, for realistic text (each word may carry an opening mark before it and
//! up to two closing marks after it, as Chinese prose does).

use std::collections::HashMap;

use chixie_rs_core::font::{Font, Glyph};
use chixie_rs_core::layout::kinsoku::{is_closing, is_opening};
use chixie_rs_core::layout::{Document, PageBox, PageParams, Ruling, layout};
use proptest::prelude::*;

const CJK: &str = "你好写字春夏秋冬";
const CLOSE: &str = "，。、”！？》";
const OPEN: &str = "“《";

fn font() -> Font {
    let glyph = Glyph { advance: 7.0, strokes: vec![vec![(1.0, 1.0), (6.0, 6.0)]] };
    let glyphs: HashMap<char, Glyph> = CJK.chars().chain(CLOSE.chars()).chain(OPEN.chars()).map(|c| (c, glyph.clone())).collect();
    Font { glyphs }
}

fn pick(set: &str, k: usize) -> char {
    let v: Vec<char> = set.chars().collect();
    v[k % v.len()]
}

/// One word: [opening] CJK [closing{0,2}], sometimes followed by a paragraph break.
fn word() -> impl Strategy<Value = String> {
    (any::<bool>(), 0usize..8, 0usize..3, 0usize..7, 0usize..7, 0u8..40).prop_map(|(open, c, n_close, a, b, nl)| {
        let mut s = String::new();
        if open {
            s.push(pick(OPEN, c));
        }
        s.push(pick(CJK, c));
        for k in 0..n_close {
            s.push(pick(CLOSE, if k == 0 { a } else { b }));
        }
        if nl == 0 {
            s.push('\n');
        }
        s
    })
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(word(), 300..400).prop_map(|w| w.concat().chars().take(1000).collect())
}

fn check(text: &str, doc: &Document) -> Result<(), TestCaseError> {
    let chars: Vec<char> = text.chars().collect();
    prop_assert_eq!(doc.boxes.len(), chars.len(), "one box per char");
    for page in &doc.pages {
        prop_assert_eq!(page.strokes.len(), page.stroke_chars.len());
    }
    for (k, l) in doc.lines.iter().enumerate() {
        let wrapped_into = l.start > 0 && l.start < chars.len() && chars[l.start - 1] != '\n';
        if wrapped_into {
            prop_assert!(!is_closing(chars[l.start]), "line {} starts with {:?}", k, chars[l.start]);
            prop_assert!(!is_opening(chars[l.start - 1]), "line {} ends with an opening mark", k - 1);
        }
    }
    prop_assert!(doc.punct_issues.is_empty(), "issues {:?}", doc.punct_issues);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn free_pages_obey_kinsoku(t in text(), width in 30.0f32..120.0) {
        let p = PageParams { page: Some(PageBox { width, height: 280.0 }), kinsoku: true, ..Default::default() };
        check(&t, &layout(&t, &font(), &p))?;
    }

    #[test]
    fn grid_pages_obey_kinsoku(t in text(), cols in 3u32..20) {
        let ruling = Ruling::Grid { rows: 20, cols, cell: (8.0, 8.0), gap: (0.0, 2.0) };
        let p = PageParams { page: Some(PageBox { width: cols as f32 * 8.0, height: 200.0 }), ruling, kinsoku: true, ..Default::default() };
        check(&t, &layout(&t, &font(), &p))?;
    }
}
