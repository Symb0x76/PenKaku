use std::collections::HashMap;

use super::*;
use crate::font::Glyph;

const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/font_fixture.json");

fn font_with(c: char, glyph: Glyph) -> Font {
    Font { glyphs: HashMap::from([(c, glyph)]) }
}

fn dot_font() -> Font {
    // '.' : one point, advance 2; 'A' : advance 3. Simple numbers for box checks.
    Font {
        glyphs: HashMap::from([
            ('.', Glyph { advance: 2.0, strokes: vec![vec![(0.0, 0.0)]] }),
            ('A', Glyph { advance: 3.0, strokes: vec![vec![(0.0, 0.0), (1.0, 1.0)]] }),
        ]),
    }
}

#[test]
fn boxes_and_lines_cover_every_char() {
    let p = PageParams { space_width: 1.0, ..Default::default() };
    let doc = layout(".A \n.x", &dot_font(), &p);
    // . A ' ' \n . x  -> 6 boxes; 'x' is missing (space width).
    assert_eq!(doc.boxes.len(), 6);
    let xs: Vec<(f32, f32)> = doc.boxes.iter().map(|b| (b.x0, b.x1)).collect();
    assert_eq!(xs, vec![(0.0, 2.0), (2.0, 5.0), (5.0, 6.0), (6.0, 6.0), (0.0, 2.0), (2.0, 3.0)]);
    assert_eq!(doc.lines.len(), 2);
    assert_eq!((doc.lines[0].start, doc.lines[0].end), (0, 3)); // ends at the '\n'
    assert_eq!((doc.lines[1].start, doc.lines[1].end), (4, 6));
    assert_eq!(doc.lines[1].top, -10.0);
}

#[test]
fn caret_and_hit_test_round_trip() {
    let doc = layout("..\n.", &dot_font(), &PageParams::default());
    // Caret x for every index; index 2 is end of line 0 (before '\n').
    let carets: Vec<(usize, f32)> = (0..=4).map(|i| (doc.caret(i).line, doc.caret(i).x)).collect();
    assert_eq!(carets, vec![(0, 0.0), (0, 2.0), (0, 4.0), (1, 0.0), (1, 2.0)]);
    // Clicking left/right halves of a glyph picks the nearer boundary.
    assert_eq!(doc.index_at(0, 0.9, -5.0), 0);
    assert_eq!(doc.index_at(0, 1.1, -5.0), 1);
    assert_eq!(doc.index_at(0, 99.0, -5.0), 2);
    assert_eq!(doc.index_at(0, 99.0, -15.0), 4);
    // Up/down keep x.
    assert_eq!(doc.vertical_move(1, 1), 4);
    assert_eq!(doc.vertical_move(4, -1), 1);
    assert_eq!(doc.vertical_move(0, -1), 0);
}

#[test]
fn wrap_boundary_caret_belongs_to_next_line() {
    let p = PageParams { page: Some(PageBox { width: 5.0, height: 100.0 }), ..Default::default() };
    let doc = layout("...", &dot_font(), &p); // 2 dots per line
    assert_eq!(doc.lines.len(), 2);
    assert_eq!((doc.lines[0].start, doc.lines[0].end), (0, 2));
    assert_eq!(doc.caret(2).line, 1);
    assert_eq!(doc.caret(2).x, 0.0);
}

#[test]
fn empty_text_has_one_line_and_origin_caret() {
    let doc = layout("", &dot_font(), &PageParams { origin: (3.0, 7.0), ..Default::default() });
    assert_eq!(doc.lines.len(), 1);
    assert_eq!(doc.caret(0).x, 3.0);
    assert_eq!(doc.caret(0).top, 7.0);
}

#[test]
fn editing_does_not_reshuffle_other_characters() {
    let font = Font::from_json(FIXTURE).unwrap();
    let p = PageParams {
        style: HandwritingStyle { enabled: true, seed: 5, ..Default::default() },
        deformation: Deformation { enabled: true, base: true, ..Default::default() },
        ..Default::default()
    };
    // Same characters with the same ids, one inserted at the front with a new id.
    let base: Vec<char> = "你好你好".chars().collect();
    let ids: Vec<u64> = vec![10, 11, 12, 13];
    let a = layout_ids(&base, &ids, &font, &p);
    let mut chars2 = vec!['A'];
    chars2.extend(&base);
    let mut ids2 = vec![99];
    ids2.extend(&ids);
    let b = layout_ids(&chars2, &ids2, &font, &p);
    // Each original glyph keeps its shape (strokes relative to the glyph's own first point);
    // only its position shifts. 'A' adds 2 strokes in front.
    let rel = |d: &Document, skip: usize| -> Vec<Vec<(i32, i32)>> {
        d.pages[0].strokes[skip..]
            .iter()
            .map(|s| s.iter().map(|&(x, y)| (((x - s[0].0) * 1000.0) as i32, ((y - s[0].1) * 1000.0) as i32)).collect())
            .collect()
    };
    assert_eq!(rel(&a, 0), rel(&b, 2));
}

#[test]
fn bounds_cover_all_pages() {
    let doc = Document {
        pages: vec![Page { strokes: vec![vec![(1.0, -2.0), (5.0, -1.0)]], ..Default::default() }, Page { strokes: vec![vec![(0.5, -9.0)]], ..Default::default() }],
        ..Default::default()
    };
    assert_eq!(doc.bounds(), Some((0.5, -9.0, 5.0, -1.0)));
    assert_eq!(doc.stroke_count(), 2);
    assert_eq!(Document::default().bounds(), None);
}

#[test]
fn flips_y_and_scales() {
    let g = Glyph { advance: 3.0, strokes: vec![vec![(0.0, 0.0), (0.0, 2.0)]] };
    let p = PageParams { origin: (10.0, 50.0), ..Default::default() };
    let doc = layout("I", &font_with('I', g), &p);
    assert_eq!(doc.pages, vec![Page { strokes: vec![vec![(10.0, 50.0), (10.0, 48.0)]], stroke_chars: vec![0], ..Default::default() }]);
}

#[test]
fn strokes_map_to_their_characters_across_pages() {
    let g = Glyph { advance: 1.0, strokes: vec![vec![(0.0, 0.0)], vec![(0.5, 0.0)]] };
    // Two lines per page: "..\n.\n." → page 0 holds chars 0,1,3; page 1 holds char 5.
    let p = PageParams { line_height: 10.0, page: Some(PageBox { width: 100.0, height: 25.0 }), ..Default::default() };
    let doc = layout("..\n.\n.", &font_with('.', g), &p);
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.pages[0].stroke_chars, vec![0, 0, 1, 1, 3, 3]);
    assert_eq!(doc.pages[1].stroke_chars, vec![5, 5]);
    for page in &doc.pages {
        assert_eq!(page.stroke_chars.len(), page.strokes.len());
    }
    assert_eq!(doc.pages[0].first_stroke_from(1), 2);
    assert_eq!(doc.pages[0].first_stroke_from(2), 4); // newline: next drawn char
    assert_eq!(doc.pages[0].first_stroke_from(4), 6); // none left on this page
}

#[test]
fn missing_chars_are_listed_once_and_advance() {
    let g = Glyph { advance: 1.0, strokes: vec![vec![(0.0, 0.0)]] };
    let p = PageParams { space_width: 4.0, ..Default::default() };
    let doc = layout("x.x", &font_with('.', g), &p);
    assert_eq!(doc.missing, vec!['x']);
    assert_eq!(doc.pages[0].strokes, vec![vec![(4.0, 0.0)]]);
}

#[test]
fn newline_resets_x_and_drops_line() {
    let g = Glyph { advance: 1.0, strokes: vec![vec![(0.0, 0.0)]] };
    let p = PageParams { origin: (0.0, 100.0), line_height: 10.0, ..Default::default() };
    let doc = layout(".\n.", &font_with('.', g), &p);
    assert_eq!(doc.pages[0].strokes, vec![vec![(0.0, 100.0)], vec![(0.0, 90.0)]]);
}

fn jitter_params(seed: u64) -> PageParams {
    PageParams {
        style: HandwritingStyle { enabled: true, seed, ..Default::default() },
        ..Default::default()
    }
}

#[test]
fn jitter_is_seeded_and_bounded() {
    // '.' is one point at (0.6, 6.9), advance 1.25 (fixture font); en spacing 0.5–1.0.
    let font = Font::from_json(FIXTURE).unwrap();
    let text = "....\n....";
    let a = layout(text, &font, &jitter_params(1));
    assert_eq!(a, layout(text, &font, &jitter_params(1)));
    assert_ne!(a, layout(text, &font, &jitter_params(2)));

    let pts: Vec<(f32, f32)> = a.pages[0].strokes.iter().map(|s| s[0]).collect();
    for line in pts.chunks(4) {
        // Line start offset 0–2 mm on every line.
        assert!((0.6..=2.6 + 1e-4).contains(&line[0].0), "start {}", line[0].0);
        for w in line.windows(2) {
            let adv = w[1].0 - w[0].0;
            assert!((1.75 - 1e-4..=2.25 + 1e-4).contains(&adv), "advance {adv}");
        }
    }
    // Vertical offset 0–0.3 below each line's nominal y (6.9 on line 0, 16.9 on line 1).
    for (i, p) in pts.iter().enumerate() {
        let nominal = if i < 4 { -6.9 } else { -16.9 };
        let off = nominal - p.1;
        assert!((-1e-4..=0.3 + 1e-4).contains(&off), "offset {off}");
    }
}

#[test]
fn deformation_alone_is_seeded_and_changes_strokes() {
    let font = Font::from_json(FIXTURE).unwrap();
    let p = PageParams {
        deformation: Deformation { enabled: true, base: true, ..Default::default() },
        ..Default::default()
    };
    let plain = layout("你好", &font, &PageParams::default());
    let d1 = layout("你好", &font, &p);
    assert_ne!(plain, d1);
    assert_eq!(d1, layout("你好", &font, &p));
    // Jitter stays off: same stroke count, no extra spacing.
    assert_eq!(plain.pages[0].strokes.len(), d1.pages[0].strokes.len());
}

#[test]
fn first_line_indent_only_on_paragraph_starts() {
    let font = Font::from_json(FIXTURE).unwrap();
    let mut p = jitter_params(4);
    p.style.line_start_offset = crate::handwriting::Span::new(0.0, 0.0);
    p.style.first_line_indent = Some(crate::handwriting::Span::new(10.0, 10.0));
    // 12.8 mm wide: after the 10 mm indent one dot fits (10 + 1.25), the second wraps
    // (cursor ≥ 11.75 + 1.25 > 12.8).
    p.page = Some(PageBox { width: 12.8, height: 100.0 });
    let doc = layout("..\n.", &font, &p);
    let xs: Vec<f32> = doc.pages[0].strokes.iter().map(|s| s[0].0).collect();
    // Paragraph start indented, wrapped line not, next paragraph indented again.
    let want = [10.6, 0.6, 10.6];
    assert!(xs.iter().zip(want).all(|(x, w)| (x - w).abs() < 1e-4), "{xs:?}");
}

#[test]
fn wraps_at_width_and_breaks_pages() {
    // advance 4 mm, box 10 x 20 mm, line height 10 → 2 glyphs/line (third would end at 12),
    // 2 lines/page (a line fits while line_top + line_height <= height).
    let g = Glyph { advance: 4.0, strokes: vec![vec![(0.0, 0.0)]] };
    let p = PageParams { page: Some(PageBox { width: 10.0, height: 20.0 }), ..Default::default() };
    let doc = layout(".....", &font_with('.', g), &p);
    let starts: Vec<Vec<(f32, f32)>> =
        doc.pages.iter().map(|pg| pg.strokes.iter().map(|s| s[0]).collect()).collect();
    assert_eq!(
        starts,
        vec![vec![(0.0, 0.0), (4.0, 0.0), (0.0, -10.0), (4.0, -10.0)], vec![(0.0, 0.0)]]
    );
}

fn cjk_font() -> Font {
    let g = Glyph { advance: 2.0, strokes: vec![vec![(0.0, 0.0)]] };
    Font { glyphs: "你好世界，。“”".chars().map(|c| (c, g.clone())).collect() }
}

fn line_texts(text: &str, doc: &Document) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    doc.lines.iter().map(|l| chars[l.start..l.end].iter().collect()).collect()
}

#[test]
fn kinsoku_pulls_the_previous_char_down() {
    // Two chars per line (width 5, advance 2).
    let p = |kinsoku| PageParams { kinsoku, ..page(5.0) };
    let off = layout("你好，世界", &cjk_font(), &p(false));
    assert_eq!(line_texts("你好，世界", &off), ["你好", "，世", "界"]);
    assert_eq!(off.punct_issues, vec![1]);
    let on = layout("你好，世界", &cjk_font(), &p(true));
    assert_eq!(line_texts("你好，世界", &on), ["你", "好，", "世界"]);
    assert!(on.punct_issues.is_empty());
    // An opening mark moves down with the char after it (three chars per line).
    let on = layout("你“好”", &cjk_font(), &PageParams { kinsoku: true, ..page(7.0) });
    assert_eq!(line_texts("你“好”", &on), ["你", "“好”"]);
    // "“好”" cannot fit two per line: break as before and report it.
    let on = layout("你“好”", &cjk_font(), &p(true));
    assert_eq!(line_texts("你“好”", &on), ["你", "“好", "”"]);
    assert_eq!(on.punct_issues, vec![2]);
}

#[test]
fn kinsoku_hangs_closing_marks_in_the_grid() {
    let p = |kinsoku| PageParams {
        page: Some(PageBox { width: 20.0, height: 100.0 }),
        ruling: Ruling::Grid { rows: 4, cols: 2, cell: (10.0, 10.0), gap: (0.0, 0.0) },
        kinsoku,
        ..Default::default()
    };
    let text = "你好。”世";
    let on = layout(text, &cjk_font(), &p(true));
    assert_eq!(line_texts(text, &on), ["你好。”", "世"]);
    // Both marks sit in the right half of the last cell, smaller.
    assert_eq!((on.boxes[2].x0, on.boxes[3].x0), (15.0, 17.5));
    assert!(on.punct_issues.is_empty());
    let off = layout(text, &cjk_font(), &p(false));
    assert_eq!(line_texts(text, &off), ["你好", "。”", "世"]);
    assert_eq!(off.punct_issues, vec![1]);
    // An opening mark never takes the last cell.
    let on = layout("你“好", &cjk_font(), &p(true));
    assert_eq!(line_texts("你“好", &on), ["你", "“好"]);
}

fn grid_params(letters: bool) -> PageParams {
    PageParams {
        page: Some(PageBox { width: 50.0, height: 100.0 }),
        ruling: Ruling::Grid { rows: 4, cols: 5, cell: (10.0, 10.0), gap: (0.0, 0.0) },
        grid_pair_letters: letters,
        ..Default::default()
    }
}

fn grid_font() -> Font {
    let g = Glyph { advance: 2.0, strokes: vec![vec![(0.0, 0.0), (2.0, 0.0)]] };
    Font { glyphs: "0123456789ab你好世界年".chars().map(|c| (c, g.clone())).collect() }
}

fn box_x0s(doc: &Document) -> Vec<f32> {
    doc.boxes.iter().map(|b| b.x0).collect()
}

#[test]
fn grid_pairs_digits_and_optionally_letters() {
    let doc = layout("2026年", &grid_font(), &grid_params(false));
    assert_eq!(box_x0s(&doc), vec![0.0, 5.0, 10.0, 15.0, 20.0]);
    // Each digit centred in its half cell.
    assert_eq!(doc.pages[0].strokes[1][0].0, 5.0 + 1.5);
    // A lone digit keeps a whole cell; letters pair only with the option.
    assert_eq!(box_x0s(&layout("年1年", &grid_font(), &grid_params(false))), vec![0.0, 10.0, 20.0]);
    assert_eq!(box_x0s(&layout("ab", &grid_font(), &grid_params(false))), vec![0.0, 10.0]);
    assert_eq!(box_x0s(&layout("ab", &grid_font(), &grid_params(true))), vec![0.0, 5.0]);
}

#[test]
fn combined_chars_share_one_cell_squeezed() {
    let chars: Vec<char> = "你好世界".chars().collect();
    let ids = [0, 1, 2, 3];
    let joins = [true, true, false, false];
    let doc = layout_flow(&Flow { chars: &chars, ids: &ids, paras: &[], joins: &joins, tuning: &[], styles: &[], deform_cache: None, blocks: &[] }, &grid_font(), &grid_params(false));
    let w = 10.0 / 3.0;
    let want = [0.0, w, 2.0 * w, 10.0];
    assert!(box_x0s(&doc).iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{:?}", box_x0s(&doc));
    // Squeezed across (2 units → 2/3 mm wide), full height.
    let st = &doc.pages[0].strokes[0];
    assert!((st[1].0 - st[0].0 - 2.0 / 3.0).abs() < 1e-4);
}

#[cfg(feature = "segment")]
#[test]
fn keep_words_moves_a_word_to_the_next_row() {
    let g = Glyph { advance: 2.0, strokes: vec![vec![(0.0, 0.0)]] };
    let font = Font { glyphs: "。我们".chars().map(|c| (c, g.clone())).collect() };
    let text = "。。。。我们";
    let mut p = grid_params(false);
    assert_eq!(line_texts(text, &layout(text, &font, &p)), ["。。。。我", "们"]);
    p.grid_keep_words = true;
    assert_eq!(line_texts(text, &layout(text, &font, &p)), ["。。。。", "我们"]);
}

#[test]
fn half_space_is_half_a_space() {
    let font = Font::from_json(FIXTURE).unwrap();
    let full = layout("你 好", &font, &jitter_params(3));
    let half = layout("你\u{2009}好", &font, &jitter_params(3));
    let w = |d: &Document| d.boxes[1].x1 - d.boxes[1].x0;
    assert!((w(&half) - w(&full) / 2.0).abs() < 1e-5, "{} vs {}", w(&half), w(&full));
}

#[test]
fn missing_width_falls_back_to_the_counterpart() {
    let g = Glyph { advance: 1.0, strokes: vec![vec![(0.0, 0.0)]] };
    let doc = layout(",Ａ", &Font { glyphs: HashMap::from([('，', g.clone()), ('A', g)]) }, &PageParams::default());
    assert!(doc.missing.is_empty());
    assert_eq!(doc.pages[0].strokes.len(), 2);
}

#[test]
fn missing_glyphs_are_located_free_and_grid() {
    let free = layout("年x年\u{2009}x", &grid_font(), &PageParams::default());
    assert_eq!((free.missing.clone(), free.missing_at.clone()), (vec!['x'], vec![1, 4]));
    let grid = layout("1\u{2009}x", &grid_font(), &grid_params(false));
    assert_eq!((grid.missing, grid.missing_at), (vec!['x'], vec![2]), "a half space in a pair is not missing");
}

fn paras(v: &[(Align, Option<f32>)]) -> Vec<Para> {
    v.iter().map(|&(align, first_indent)| Para { align, first_indent }).collect()
}

fn lay(text: &str, ps: &[Para], p: &PageParams) -> Document {
    let chars: Vec<char> = text.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    layout_doc(&chars, &ids, ps, &dot_font(), p)
}

fn starts(doc: &Document) -> Vec<f32> {
    doc.pages[0].strokes.iter().map(|s| s[0].0).collect()
}

fn page(width: f32) -> PageParams {
    PageParams { page: Some(PageBox { width, height: 100.0 }), ..Default::default() }
}

#[test]
fn right_and_centre_shift_the_line() {
    let right = lay("..", &paras(&[(Align::Right, None)]), &page(10.0));
    assert_eq!(starts(&right), vec![6.0, 8.0]);
    assert_eq!(right.boxes[1].x1, 10.0);
    let centre = lay("..", &paras(&[(Align::Center, None)]), &page(10.0));
    assert_eq!(starts(&centre), vec![3.0, 5.0]);
    // Trailing spaces hang past the margin instead of pushing the text left.
    let spaced = lay(". ", &paras(&[(Align::Right, None)]), &PageParams { space_width: 1.0, ..page(10.0) });
    assert_eq!(starts(&spaced), vec![8.0]);
}

#[test]
fn justify_stretches_wrapped_lines_only() {
    // Four dots fit in 9 mm (8 mm used), the fifth wraps; the last line stays left.
    let doc = lay(".....", &paras(&[(Align::Justify, None)]), &page(9.0));
    let xs = starts(&doc);
    let want = [0.0, 2.0 + 1.0 / 3.0, 4.0 + 2.0 / 3.0, 7.0, 0.0];
    assert!(xs.iter().zip(want).all(|(x, w)| (x - w).abs() < 1e-4), "{xs:?}");
    assert!((doc.boxes[3].x1 - 9.0).abs() < 1e-4);
    // A line ended by '\n' is not stretched.
    let doc = lay("..\n.", &paras(&[(Align::Justify, None)]), &page(9.0));
    assert_eq!(starts(&doc), vec![0.0, 2.0, 0.0]);
}

#[test]
fn alignment_is_per_paragraph() {
    let doc = lay(".\n.\n.", &paras(&[(Align::Right, None), (Align::Left, None), (Align::Center, None)]), &page(10.0));
    assert_eq!(starts(&doc), vec![8.0, 0.0, 4.0]);
}

#[test]
fn right_aligned_jittered_line_ends_at_the_margin() {
    let font = Font::from_json(FIXTURE).unwrap();
    let mut p = jitter_params(9);
    p.page = Some(PageBox { width: 100.0, height: 100.0 });
    let chars: Vec<char> = "你好你好".chars().collect();
    let ids: Vec<u64> = (0..4).collect();
    let doc = layout_doc(&chars, &ids, &paras(&[(Align::Right, None)]), &font, &p);
    assert!((doc.boxes[3].x1 - 100.0).abs() < 1e-3, "{}", doc.boxes[3].x1);
    let max_x = doc.pages[0].strokes.iter().flatten().map(|pt| pt.0).fold(f32::MIN, f32::max);
    assert!(max_x <= 100.0 && max_x > 100.0 - 3.0, "{max_x}");
}

#[test]
fn explicit_indent_applies_to_the_first_line_only() {
    // Jitter off: 5 mm indent on paragraph 0; width 8 → one dot fits after the indent.
    let doc = lay("..\n.", &paras(&[(Align::Left, Some(5.0)), (Align::Left, None)]), &page(8.0));
    assert_eq!(starts(&doc), vec![5.0, 0.0, 0.0]);
}

#[test]
fn grid_aligns_by_whole_cells_and_indents_two_cells() {
    let grid = |align, indent| {
        let p = PageParams {
            page: Some(PageBox { width: 25.0, height: 100.0 }),
            ruling: Ruling::Grid { rows: 4, cols: 5, cell: (4.0, 4.0), gap: (1.0, 1.0) },
            ..Default::default()
        };
        lay(".", &paras(&[(align, indent)]), &p)
    };
    assert_eq!(grid(Align::Right, None).boxes[0].x0, 20.0);
    assert_eq!(grid(Align::Center, None).boxes[0].x0, 10.0);
    assert_eq!(grid(Align::Justify, None).boxes[0].x0, 0.0);
    assert_eq!(grid(Align::Left, Some(16.0)).boxes[0].x0, 10.0);
}

#[test]
fn tuning_moves_resizes_and_rerolls_one_char() {
    let font = Font::from_json(FIXTURE).unwrap();
    let chars: Vec<char> = "你好你".chars().collect();
    let ids = [1, 2, 3];
    let p = PageParams { deformation: Deformation { enabled: true, base: true, ..Default::default() }, ..Default::default() };
    let lay = |t: Option<Tuning>| layout_flow(&Flow { chars: &chars, ids: &ids, paras: &[], joins: &[], tuning: &[None, t, None], styles: &[], deform_cache: None, blocks: &[] }, &font, &p);
    let plain = lay(None);
    let ink = |d: &Document, c: usize| -> Vec<(f32, f32)> {
        d.pages[0].strokes.iter().zip(&d.pages[0].stroke_chars).filter(|(_, k)| **k == c).flat_map(|(s, _)| s.iter().copied()).collect()
    };
    // Nudge: exactly that char moves; the others and the boxes do not.
    let moved = lay(Some(Tuning { dx_mm: 0.4, dy_mm: -0.2, ..Default::default() }));
    for (a, b) in ink(&plain, 1).iter().zip(ink(&moved, 1)) {
        assert!((b.0 - a.0 - 0.4).abs() < 1e-4 && (b.1 - a.1 - 0.2).abs() < 1e-4, "{a:?} {b:?}");
    }
    assert_eq!((ink(&plain, 0), ink(&plain, 2)), (ink(&moved, 0), ink(&moved, 2)));
    assert_eq!(plain.boxes, moved.boxes);
    // Bigger and narrower: the bottom (glyph y = 7 units) stays, the width shrinks.
    let span = |v: &[(f32, f32)]| {
        let (x0, x1) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (y0, y1) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        (x1 - x0, y1 - y0)
    };
    let big = lay(Some(Tuning { scale: 1.1, width: 0.9, ..Default::default() }));
    let ((w0, h0), (w1, h1)) = (span(&ink(&plain, 1)), span(&ink(&big, 1)));
    assert!((h1 / h0 - 1.1).abs() < 0.01 && (w1 / w0 - 0.99).abs() < 0.01, "{w0} {h0} -> {w1} {h1}");
    // 变形: another deformation, same place.
    let rerolled = lay(Some(Tuning { reroll: 1, ..Default::default() }));
    assert_ne!(ink(&plain, 1), ink(&rerolled, 1));
    assert_eq!(plain.boxes, rerolled.boxes);
}

/// 'X': advance 5, one stroke spanning the em (0..7 units).
fn x_font() -> Font {
    font_with('X', Glyph { advance: 5.0, strokes: vec![vec![(0.0, 0.0), (5.0, 7.0)]] })
}

fn table(cells: Vec<CellBox>) -> PageParams {
    PageParams { page: Some(PageBox { width: 100.0, height: 100.0 }), ruling: Ruling::Table { pages: vec![cells] }, ..Default::default() }
}

/// 2×2 cells of 20 × 10 mm, one row each, 1 mm side padding.
fn two_by_two(overflow: Overflow) -> Vec<CellBox> {
    let c = |x: f32, y: f32| CellBox { x, y, w: 20.0, h: 10.0, rows: 1, overflow, ..Default::default() };
    vec![c(0.0, 0.0), c(20.0, 0.0), c(0.0, 10.0), c(20.0, 10.0)]
}

fn x_span(s: &Stroke) -> (f32, f32) {
    s.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)))
}

#[test]
fn table_segments_fill_cells_then_the_next_page() {
    let b = CELL_BREAK;
    let doc = layout(&format!("X{b}X{b}X{b}X{b}X"), &x_font(), &table(two_by_two(Overflow::Clip)));
    assert_eq!(doc.pages.len(), 2);
    let segs: Vec<(usize, usize, usize)> = doc.cells.iter().map(|c| (c.page, c.cell, c.seg)).collect();
    assert_eq!(segs, vec![(0, 0, 0), (0, 1, 1), (0, 2, 2), (0, 3, 3), (1, 0, 4)]);
    // Each X starts inside its cell's padding and stays inside the cell vertically.
    for (k, s) in doc.pages[0].strokes.iter().enumerate() {
        let cx = if k % 2 == 0 { 0.0 } else { 20.0 };
        assert_eq!(x_span(s).0, cx + 1.0);
        let (top, bottom) = (s.iter().map(|p| p.1).fold(f32::MIN, f32::max), s.iter().map(|p| p.1).fold(f32::MAX, f32::min));
        let cell_bottom = if k < 2 { -10.0 } else { -20.0 };
        assert!(top <= cell_bottom + 10.0 && bottom >= cell_bottom, "stroke {k} y {top}..{bottom}");
    }
    assert_eq!(doc.pages[1].guides.len(), 12, "a 2×2 table has 12 distinct border segments");
    assert_eq!(doc.overflowing().count(), 0);
}

#[test]
fn clip_drops_what_does_not_fit() {
    // 18 mm inside, X is 5 mm: three per row, one row.
    let doc = layout("XXXXXX", &x_font(), &table(two_by_two(Overflow::Clip)));
    assert_eq!(doc.pages[0].strokes.len(), 3);
    assert_eq!(doc.pages[0].stroke_chars, vec![0, 1, 2]);
    assert_eq!(doc.cells[0].overflow_from, Some(3));
    assert_eq!(doc.boxes.len(), 6, "overflowing chars keep their caret boxes");
}

#[test]
fn shrink_fits_forty_chars_into_a_two_row_cell() {
    let cell = CellBox { w: 20.0, h: 20.0, rows: 2, overflow: Overflow::Shrink { min: 0.1 }, ..Default::default() };
    let doc = layout(&"X".repeat(40), &x_font(), &table(vec![cell]));
    let c = doc.cells[0];
    assert_eq!(c.overflow_from, None, "fits after shrinking");
    assert!(c.fit.scale < 0.2 && c.fit.scale >= 0.1, "scale {}", c.fit.scale);
    assert_eq!(doc.pages[0].strokes.len(), 40);
    for s in &doc.pages[0].strokes {
        assert!(x_span(s).1 <= 19.0 + 1e-3);
    }
}

#[test]
fn shrink_stops_at_its_minimum() {
    let cell = CellBox { w: 20.0, h: 10.0, rows: 1, overflow: Overflow::Shrink { min: 0.9 }, ..Default::default() };
    let doc = layout("XXXXXX", &x_font(), &table(vec![cell]));
    assert!(doc.cells[0].overflow_from.is_some());
    assert!((doc.cells[0].fit.scale - 0.9).abs() < 1e-6);
}

#[test]
fn add_rows_takes_the_fewest_that_fit() {
    let cell = CellBox { w: 20.0, h: 10.0, rows: 1, overflow: Overflow::AddRows { max: 4 }, ..Default::default() };
    let doc = layout("XXXXXX", &x_font(), &table(vec![cell]));
    // Two rows at half size: 7 chars a row.
    assert_eq!(doc.cells[0].fit, CellFit { scale: 0.5, rows: 2 });
    assert_eq!(doc.cells[0].overflow_from, None);
    assert_eq!(doc.pages[0].strokes.len(), 6);
}

#[test]
fn middle_alignment_centres_the_lines() {
    let cell = CellBox { w: 20.0, h: 30.0, rows: 3, valign: VAlign::Middle, ..Default::default() };
    let top = |d: &Document| d.pages[0].strokes[0].iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let mid = layout("X", &x_font(), &table(vec![cell]));
    let up = layout("X", &x_font(), &table(vec![CellBox { valign: VAlign::Top, ..cell }]));
    assert!((top(&up) - top(&mid) - 10.0).abs() < 1e-4, "one row lower");
    assert_eq!(mid.lines[0].top, -10.0);
}

#[test]
fn cell_break_is_a_line_break_outside_tables() {
    let doc = layout(&format!("X{CELL_BREAK}X"), &x_font(), &PageParams::default());
    assert_eq!(doc.lines.len(), 2);
    assert!(doc.cells.is_empty());
}

fn styled(text: &str, styles: &[Option<CharStyle>], font: &(impl Glyphs + ?Sized), p: &PageParams) -> Document {
    let chars: Vec<char> = text.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    layout_flow(&Flow { chars: &chars, ids: &ids, styles, ..Default::default() }, font, p)
}

#[test]
fn double_size_char_advances_twice_as_far() {
    let big = Some(CharStyle { scale: 2.0, ..Default::default() });
    let d = styled("XXX", &[None, big, None], &x_font(), &PageParams::default());
    let w: Vec<f32> = d.boxes.iter().map(|b| b.x1 - b.x0).collect();
    assert_eq!(w, vec![5.0, 10.0, 5.0]);
    // Its foot stays on the line: the stroke ends at the plain char's foot height.
    let foot = |k: usize| d.pages[0].strokes[k][1].1;
    assert!((foot(1) - foot(0)).abs() < 1e-4, "{} {}", foot(0), foot(1));
}

#[test]
fn plain_styles_change_nothing() {
    let p = PageParams::default();
    let a = layout("XX X", &x_font(), &p);
    let b = styled("XX X", &[Some(CharStyle::default()); 4], &x_font(), &p);
    assert_eq!(a, b);
}

#[test]
fn underline_adds_one_stroke_spanning_the_run() {
    let u = Some(CharStyle { underline: true, ..Default::default() });
    let d = styled("XXXX", &[None, u, u, None], &x_font(), &PageParams::default());
    let page = &d.pages[0];
    assert_eq!(page.strokes.len(), 5);
    let line = page.strokes.last().unwrap();
    assert_eq!((line[0].0, line.last().unwrap().0), (d.boxes[1].x0, d.boxes[2].x1));
    // Tagged to the run's last char, below the glyph foot.
    assert_eq!(*page.stroke_chars.last().unwrap(), 2);
    assert!(line[0].1 < page.strokes[0][1].1);
    // Strike-through sits inside the glyph's height.
    let s = Some(CharStyle { strike: true, ..Default::default() });
    let d = styled("XX", &[s, s], &x_font(), &PageParams::default());
    let y = d.pages[0].strokes.last().unwrap()[0].1;
    assert!(y < 0.0 && y > -7.0, "{y}");
}

#[test]
fn underline_breaks_at_line_ends() {
    let u = Some(CharStyle { underline: true, ..Default::default() });
    let p = PageParams { page: Some(PageBox { width: 11.0, height: 100.0 }), ..Default::default() };
    let d = styled("XXXX", &[u; 4], &x_font(), &p);
    // Two lines of two chars: one rule each.
    assert_eq!(d.lines.len(), 2);
    assert_eq!(d.pages[0].strokes.len(), 6);
}

#[test]
fn tilt_keeps_the_foot_and_moves_the_top() {
    let r = Some(CharStyle { tilt: 1, ..Default::default() });
    let plain = layout("X", &x_font(), &PageParams::default());
    let lean = styled("X", &[r], &x_font(), &PageParams::default());
    let (a, b) = (&plain.pages[0].strokes[0], &lean.pages[0].strokes[0]);
    // (0,0) is the top: moved right by tan(12°) × 7; (5,7) is the foot: unchanged.
    assert!((b[0].0 - a[0].0 - 7.0 * TILT_DEG.to_radians().tan()).abs() < 1e-4);
    assert_eq!(a[0].1, b[0].1);
    assert_eq!(a[1], b[1]);
    let l = styled("X", &[Some(CharStyle { tilt: -1, ..Default::default() })], &x_font(), &PageParams::default());
    assert!(l.pages[0].strokes[0][0].0 < a[0].0);
}

#[test]
fn scripts_shrink_and_shift() {
    let plain = layout("X", &x_font(), &PageParams::default());
    let sup = styled("X", &[Some(CharStyle { script: Script::Super, ..Default::default() })], &x_font(), &PageParams::default());
    let sub = styled("X", &[Some(CharStyle { script: Script::Sub, ..Default::default() })], &x_font(), &PageParams::default());
    let foot = |d: &Document| d.pages[0].strokes[0][1].1;
    // y is up: superscript foot above the line, subscript foot below it.
    assert!(foot(&sup) > foot(&plain) && foot(&sub) < foot(&plain));
    assert!((sup.boxes[0].x1 - SCRIPT_SCALE * 5.0).abs() < 1e-4);
    let title = styled("X", &[Some(CharStyle { title: true, ..Default::default() })], &x_font(), &PageParams::default());
    assert!((title.boxes[0].x1 - TITLE_SCALE * 5.0).abs() < 1e-4);
}

#[test]
fn run_font_override_picks_that_fonts_glyph() {
    use crate::font::fontset::RunFonts;
    let base = x_font();
    let other = std::sync::Arc::new(font_with('X', Glyph { advance: 9.0, strokes: vec![vec![(0.0, 0.0)]] }));
    let fonts = RunFonts { base: &base, by_id: HashMap::from([(1, other)]) };
    let d = layout("XX", &fonts, &PageParams::default());
    assert_eq!(d.boxes[0].x1 - d.boxes[0].x0, 5.0);
    assert_eq!(d.boxes[1].x1 - d.boxes[1].x0, 9.0);
}

#[test]
fn deform_cache_matches_uncached_layout() {
    let font = Font::from_json(FIXTURE).unwrap();
    let mut p = PageParams {
        style: HandwritingStyle { enabled: true, seed: 7, ..Default::default() },
        deformation: Deformation { enabled: true, base: true, shape: true, ..Default::default() },
        ..Default::default()
    };
    let cache = DeformCache::new();
    let lay = |text: &str, p: &PageParams, cache: Option<&DeformCache>| {
        let chars: Vec<char> = text.chars().collect();
        let ids: Vec<u64> = (0..chars.len() as u64).collect();
        layout_flow(&Flow { chars: &chars, ids: &ids, deform_cache: cache, ..Default::default() }, &font, p)
    };
    let text = "你好A你好.\n好你A";
    assert_eq!(lay(text, &p, Some(&cache)), lay(text, &p, None));
    let filled = cache.len();
    assert!(filled > 0);
    // An edit reuses the entries of untouched chars and still matches.
    let edited = "你好A你好.\n好你A好";
    assert_eq!(lay(edited, &p, Some(&cache)), lay(edited, &p, None));
    assert_eq!(cache.len(), filled + 1);
    // New deformation settings start the cache over.
    p.deformation.shape_degree += 1.0;
    assert_eq!(lay(text, &p, Some(&cache)), lay(text, &p, None));
    assert!(cache.len() <= filled);
}

#[test]
fn cursive_joins_strokes_within_chars_only() {
    let font = Font::from_json(FIXTURE).unwrap();
    let lay = |p: &PageParams, cache: Option<&DeformCache>| {
        let chars: Vec<char> = "你好你好".chars().collect();
        let ids: Vec<u64> = (0..chars.len() as u64).collect();
        layout_flow(&Flow { chars: &chars, ids: &ids, deform_cache: cache, ..Default::default() }, &font, p)
    };
    let off = lay(&PageParams::default(), None);
    let mut p = PageParams::default();
    p.style.cursive = crate::handwriting::Cursive { enabled: true, max_gap_pct: 100.0, chance_pct: 100.0 };
    let on = lay(&p, None);
    let (n_off, n_on) = (off.pages[0].strokes.len(), on.pages[0].strokes.len());
    assert!(n_on < n_off, "{n_on} strokes joined from {n_off}");
    // Joins stay inside a char: every char still has its own strokes, in the same order.
    let chars_of = |d: &Document| {
        let mut v = d.pages[0].stroke_chars.clone();
        v.dedup();
        v
    };
    assert_eq!(chars_of(&on), chars_of(&off));
    // Cached and uncached agree; the switch is part of the cache's fingerprint.
    let cache = DeformCache::new();
    assert_eq!(lay(&p, Some(&cache)), on);
    p.style.cursive.enabled = false;
    assert_eq!(lay(&p, Some(&cache)), off);
}

fn script_params(f: impl FnOnce(&mut crate::handwriting::ScriptStyle)) -> PageParams {
    let mut p = PageParams::default();
    f(&mut p.style.english);
    p
}

#[test]
fn english_scale_shrinks_letters_and_their_advance() {
    let font = Font::from_json(FIXTURE).unwrap();
    let plain = layout("AA你", &font, &PageParams::default());
    let small = layout("AA你", &font, &script_params(|s| s.scale_pct = 80.0));
    let adv = |d: &Document| d.boxes[1].x0 - d.boxes[0].x0;
    assert!((adv(&small) - 0.8 * adv(&plain)).abs() < 1e-4, "{} vs {}", adv(&small), adv(&plain));
    // 你 keeps its size: same stroke shape, only moved left.
    let shape = |d: &Document, k: usize| {
        let s = &d.pages[0].strokes;
        let ix: Vec<usize> = (0..s.len()).filter(|&i| d.pages[0].stroke_chars[i] == k).collect();
        let o = s[ix[0]][0];
        ix.iter().flat_map(|&i| s[i].iter().map(move |&(x, y)| (x - o.0, y - o.1))).collect::<Vec<_>>()
    };
    let (a, b) = (shape(&plain, 2), shape(&small, 2));
    assert!(a.iter().zip(&b).all(|(p, q)| (p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4));
}

#[test]
fn english_baseline_raises_only_letters() {
    let font = Font::from_json(FIXTURE).unwrap();
    let plain = layout("你A", &font, &PageParams::default());
    let raised = layout("你A", &font, &script_params(|s| s.baseline_mm = 2.0));
    let pts = |d: &Document, k: usize| -> Vec<(f32, f32)> {
        let pg = &d.pages[0];
        (0..pg.strokes.len()).filter(|&i| pg.stroke_chars[i] == k).flat_map(|i| pg.strokes[i].clone()).collect()
    };
    assert_eq!(pts(&plain, 0), pts(&raised, 0));
    for (p, q) in pts(&plain, 1).iter().zip(pts(&raised, 1)) {
        assert!((q.1 - p.1 - 2.0).abs() < 1e-4 && (q.0 - p.0).abs() < 1e-4);
    }
}

#[test]
fn english_only_deformation_leaves_chinese_alone() {
    let font = Font::from_json(FIXTURE).unwrap();
    let on = Deformation { enabled: true, base: true, shape: true, ..Default::default() };
    let plain = layout("你A你", &font, &PageParams::default());
    let de = layout("你A你", &font, &script_params(|s| s.deformation = Some(on)));
    let of = |d: &Document, k: usize| -> Vec<Vec<(f32, f32)>> {
        let pg = &d.pages[0];
        (0..pg.strokes.len()).filter(|&i| pg.stroke_chars[i] == k).map(|i| pg.strokes[i].clone()).collect()
    };
    assert_eq!(of(&plain, 0), of(&de, 0));
    assert_eq!(of(&plain, 2), of(&de, 2));
    assert_ne!(of(&plain, 1), of(&de, 1));
}
