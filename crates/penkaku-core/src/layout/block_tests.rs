//! Inline tables and flowcharts in the text flow.

use std::collections::HashMap;

use super::*;
use crate::handwriting::HandwritingStyle;

/// 'X': advance 5, one stroke spanning the em (0..7 units).
fn x_font() -> Font {
    Font { glyphs: HashMap::from([('X', Glyph { advance: 5.0, strokes: vec![vec![(0.0, 0.0), (5.0, 7.0)]] })]) }
}

fn with_blocks(text: &str, blocks: &[(usize, Block)], ps: &[Para], p: &PageParams) -> Document {
    let chars: Vec<char> = text.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    let mut bl: Vec<Option<&Block>> = vec![None; chars.len()];
    for (k, b) in blocks {
        bl[*k] = Some(b);
    }
    layout_flow(&Flow { chars: &chars, ids: &ids, paras: ps, blocks: &bl, ..Default::default() }, &x_font(), p)
}

fn tall_page(h: f32) -> PageParams {
    PageParams { page: Some(PageBox { width: 100.0, height: h }), ..Default::default() }
}

/// Strokes attributed to char `k`, on every page.
fn tagged(doc: &Document, k: usize) -> Vec<&Stroke> {
    doc.pages.iter().flat_map(|p| p.strokes.iter().zip(&p.stroke_chars).filter(|(_, c)| **c == k).map(|(s, _)| s)).collect()
}

fn x_span(s: &Stroke) -> (f32, f32) {
    s.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)))
}

/// Vertical extent in mm down the page (origin at 0).
fn y_span(s: &Stroke) -> (f32, f32) {
    s.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(-p.1), b.max(-p.1)))
}

/// "X" ¶ [X | XX] [X | ·] ¶ "X": a 2-column table after one line of text.
fn small_table() -> (String, Vec<(usize, Block)>) {
    let t = format!("X\n{OBJECT}X{CELL_BREAK}XX{CELL_BREAK}X{CELL_BREAK}{TABLE_END}\nX");
    (t, vec![(2, Block::Table { cols: vec![30.0, 30.0] })])
}

#[test]
fn inline_table_draws_its_frame_and_puts_cells_in_place() {
    let (t, b) = small_table();
    let doc = with_blocks(&t, &b, &[], &tall_page(200.0));
    let pg = &doc.pages[0];
    assert_eq!(pg.strokes.len(), pg.stroke_chars.len());
    assert_eq!(doc.boxes.len(), t.chars().count(), "one box per char");
    // 2 rows: 3 row edges + 3 column edges, straight with handwriting off.
    let frame = tagged(&doc, 2);
    assert_eq!(frame.len(), 6);
    assert!(frame.iter().all(|s| s.len() == 2));
    // The table starts on the second line (10 mm down); rows are one line (10 mm) tall.
    let (top, bottom) = frame.iter().fold((f32::MAX, f32::MIN), |(a, b), s| (a.min(y_span(s).0), b.max(y_span(s).1)));
    assert_eq!((top, bottom), (10.0, 30.0));
    let inside = |k: usize, x: (f32, f32), y: (f32, f32)| {
        let v = tagged(&doc, k);
        !v.is_empty()
            && v.iter().all(|s| {
                let (sx, sy) = (x_span(s), y_span(s));
                sx.0 >= x.0 && sx.1 <= x.1 && sy.0 >= y.0 && sy.1 <= y.1
            })
    };
    assert!(inside(3, (0.0, 30.0), (10.0, 20.0)), "cell 1 in row 1, column 1");
    assert!(inside(5, (30.0, 60.0), (10.0, 20.0)) && inside(6, (30.0, 60.0), (10.0, 20.0)), "cell 2 in column 2");
    assert!(inside(8, (0.0, 30.0), (20.0, 30.0)), "cell 3 in row 2");
    // Text after the table starts below it.
    let after = t.chars().count() - 1;
    assert!(tagged(&doc, after).iter().all(|s| y_span(s).0 > 30.0));
    // Carets: cell chars sit in their column; the last char is on the last line.
    assert!(doc.boxes[5].x0 >= 30.0 && doc.boxes[5].x1 <= 60.0);
    assert_eq!(doc.line_of(after), doc.lines.len() - 1);
    for w in doc.lines.windows(2) {
        assert!(w[0].start <= w[1].start, "lines in text order: {:?}", doc.lines);
    }
}

#[test]
fn inline_table_cells_wrap_and_grow_their_row() {
    // 8 × 5 mm of text in a 20 mm column (≈ 15.8 mm writable): 3 lines → a 30 mm row.
    let t = format!("{OBJECT}XXXXXXXX{CELL_BREAK}X{TABLE_END}");
    let doc = with_blocks(&t, &[(0, Block::Table { cols: vec![20.0, 20.0] })], &[], &tall_page(200.0));
    let bottom = tagged(&doc, 0).iter().map(|s| y_span(s).1).fold(f32::MIN, f32::max);
    assert_eq!(bottom, 30.0);
    assert_eq!(doc.lines.iter().filter(|l| (1..=8).contains(&l.start)).count(), 3);
}

#[test]
fn inline_table_rows_move_to_the_next_page() {
    // Four 10 mm rows on a 25 mm page: two fit, the rest go over.
    let t = format!("{OBJECT}X{CELL_BREAK}X{CELL_BREAK}X{CELL_BREAK}X{TABLE_END}");
    let doc = with_blocks(&t, &[(0, Block::Table { cols: vec![30.0] })], &[], &tall_page(25.0));
    assert_eq!(doc.pages.len(), 2);
    for p in &doc.pages {
        assert_eq!(p.strokes.len(), p.stroke_chars.len());
    }
    // Each page part has its own frame: 3 row edges + 2 column edges.
    assert_eq!(doc.pages[0].stroke_chars.iter().filter(|&&c| c == 0).count(), 5);
    assert_eq!(doc.pages[1].stroke_chars.iter().filter(|&&c| c == 0).count(), 5);
}

/// Every stroke of every page lies within the page's height (mm down from the top).
fn all_on_paper(doc: &Document, h: f32) -> bool {
    doc.pages.iter().flat_map(|p| &p.strokes).all(|s| {
        let (a, b) = y_span(s);
        a >= -1e-3 && b <= h + 1e-3
    })
}

/// "X" ¶ one cell of 40 X's (8 lines in a 30 mm column) ¶ "X" ¶ "X".
fn long_cell() -> (String, Vec<(usize, Block)>) {
    let long = "X".repeat(40);
    (format!("X\n{OBJECT}{long}{TABLE_END}\nX\nX"), vec![(2, Block::Table { cols: vec![30.0] })])
}

#[test]
fn ruled_table_row_taller_than_the_page_goes_on_overleaf() {
    // 信纸 with 3 rows a page; the cell needs 8 (a long cell, as in an imported report). Its
    // row fills the rest of page 0 (2 rows), all of page 1 and page 2, each part framed on its
    // own page; nothing runs past a page's foot, and the text after it starts on page 3.
    let p = PageParams {
        page: Some(PageBox { width: 100.0, height: 30.0 }),
        ruling: Ruling::Rows { rows: (3, 3), columns: 1, column_gap: 0.0, widths: vec![], band: None, upper: false, dashed: false, divider: false },
        ..Default::default()
    };
    let (t, b) = long_cell();
    let doc = with_blocks(&t, &b, &[], &p);
    assert_eq!(doc.boxes.len(), t.chars().count(), "one box per char");
    assert!(all_on_paper(&doc, 30.0));
    let framed = |pg: usize| doc.pages[pg].stroke_chars.contains(&2);
    assert!(framed(0) && framed(1) && framed(2));
    let cell_lines: Vec<usize> = doc.lines.iter().filter(|l| l.start >= 3 && l.end <= 43).map(|l| l.page).collect();
    assert_eq!(cell_lines, vec![0, 0, 1, 1, 1, 2, 2, 2]);
    let last = t.chars().count() - 1;
    assert!(doc.lines.last().is_some_and(|l| l.page == 3 && l.end >= last));
}

#[test]
fn free_table_row_taller_than_the_page_goes_on_overleaf() {
    let (t, b) = long_cell();
    let doc = with_blocks(&t, &b, &[], &tall_page(30.0));
    assert_eq!(doc.boxes.len(), t.chars().count(), "one box per char");
    assert!(all_on_paper(&doc, 30.0));
    assert!(doc.pages.len() >= 3);
}

fn picture(w: f32, h: f32, number: u32) -> Block {
    Block::Picture { w_mm: w, h_mm: h, number, media: format!("image{number}.png"), data: ImageData::default() }
}

#[test]
fn picture_leaves_its_space_with_its_number_and_text_goes_on_below() {
    // "X" ¶ picture 40 × 25 mm ¶ "X", centred paragraph for the picture.
    let t = format!("X\n{OBJECT}\nX");
    let ps = [Para::default(), Para { align: Align::Center, ..Para::default() }, Para::default()];
    let mut font = x_font();
    for c in ['图', '3'] {
        font.glyphs.insert(c, Glyph { advance: 7.0, strokes: vec![vec![(0.0, 0.0), (7.0, 7.0)]] });
    }
    let chars: Vec<char> = t.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    let pic = picture(40.0, 25.0, 3);
    let mut bl: Vec<Option<&Block>> = vec![None; chars.len()];
    bl[2] = Some(&pic);
    let doc = layout_flow(&Flow { chars: &chars, ids: &ids, paras: &ps, blocks: &bl, ..Default::default() }, &font, &tall_page(200.0));
    assert_eq!(doc.boxes.len(), t.chars().count());
    let slot = &doc.pictures[0];
    assert_eq!((slot.number, slot.char_ix, slot.page), (3, 2, 0));
    assert_eq!((slot.w, slot.h), (40.0, 25.0));
    assert!((slot.x - 30.0).abs() < 1e-3, "centred on the 100 mm line: {}", slot.x);
    // 「图3」: two strokes tagged to the anchor, in the box's top-left corner.
    let (top, below) = (-slot.top, -(slot.top - slot.h));
    let label = tagged(&doc, 2);
    assert_eq!(label.len(), 2);
    assert!(label.iter().all(|s| x_span(s).0 >= slot.x && x_span(s).1 <= slot.x + slot.w / 2.0 && y_span(s).0 >= top && y_span(s).1 <= top + slot.h / 2.0));
    // The text after the picture starts below its space.
    assert!(tagged(&doc, 4).iter().all(|s| y_span(s).0 >= below - 1e-3));
}

#[test]
fn picture_shrinks_to_the_line_and_keeps_its_shape() {
    let doc = with_blocks(&format!("{OBJECT}"), &[(0, picture(200.0, 100.0, 1))], &[], &tall_page(400.0));
    let s = &doc.pictures[0];
    assert!((s.w - 100.0).abs() < 1e-3 && (s.h - 50.0).abs() < 1e-3, "{s:?}");
}

#[test]
fn pictures_in_table_cells_stay_side_by_side() {
    // A 2-column table, a 50 × 30 picture in each cell.
    let t = format!("{OBJECT}{OBJECT}{CELL_BREAK}{OBJECT}{TABLE_END}");
    let b = vec![(0, Block::Table { cols: vec![60.0, 60.0] }), (1, picture(50.0, 30.0, 1)), (3, picture(50.0, 30.0, 2))];
    let doc = with_blocks(&t, &b, &[], &tall_page(200.0));
    let p: Vec<(u32, usize)> = doc.pictures.iter().map(|s| (s.number, s.char_ix)).collect();
    assert_eq!(p, vec![(1, 1), (2, 3)]);
    let (a, c) = (&doc.pictures[0], &doc.pictures[1]);
    assert!((a.top - c.top).abs() < 1e-3, "same row: {a:?} {c:?}");
    assert!(c.x >= a.x + a.w, "second to the right of the first");
    assert!(a.w <= 50.0 + 1e-3 && a.h > 20.0);
    // The row grows to hold the pictures: the table's frame reaches below them.
    let frame_bottom = tagged(&doc, 0).iter().map(|s| y_span(s).1).fold(0.0f32, f32::max);
    assert!(frame_bottom >= -(a.top - a.h) - 1e-3, "frame {frame_bottom} vs picture foot {}", -(a.top - a.h));
}

#[test]
fn inline_table_frame_wavers_with_handwriting_but_keeps_corners() {
    let (t, b) = small_table();
    let p = PageParams { page: Some(PageBox { width: 100.0, height: 200.0 }), style: HandwritingStyle { enabled: true, seed: 3, ..Default::default() }, ..Default::default() };
    let doc = with_blocks(&t, &b, &[], &p);
    let frame = tagged(&doc, 2);
    assert_eq!(frame.len(), 6);
    assert!(frame.iter().any(|s| s.len() > 2), "long lines are drawn in wobbly pieces");
    let x0 = doc.boxes[2].x0;
    let top = frame.iter().map(|s| y_span(s).0).fold(f32::MAX, f32::min);
    for s in &frame {
        let (a, z) = (s[0], s[s.len() - 1]);
        // Ends exactly on a column edge or a row edge.
        let on = |q: (f32, f32)| [x0, x0 + 30.0, x0 + 60.0].contains(&q.0) || [top, top + 10.0, top + 20.0].contains(&-q.1);
        assert!(on(a) && on(z), "{a:?} {z:?}");
    }
}

#[test]
fn centred_table_moves_right() {
    let (t, b) = small_table();
    let ps = vec![Para::default(), Para { align: Align::Center, first_indent: None }, Para::default()];
    let doc = with_blocks(&t, &b, &ps, &tall_page(200.0));
    let left = tagged(&doc, 2).iter().map(|s| x_span(s).0).fold(f32::MAX, f32::min);
    assert_eq!(left, 20.0);
}

#[test]
fn broken_markers_draw_nothing_and_never_panic() {
    let doc = with_blocks(&format!("X{TABLE_END}X{OBJECT}X"), &[(3, Block::Table { cols: vec![10.0] })], &[], &tall_page(200.0));
    assert!(doc.missing.is_empty());
    assert_eq!(doc.boxes.len(), 5);
    assert_eq!(doc.stroke_count(), 3);
}

#[test]
fn flowchart_draws_shapes_links_and_labels_then_text_continues_below() {
    let t = format!("X\n{OBJECT}\nX");
    let b = vec![(2, Block::Diagram { source: "graph TD\nA[X] --> B[XX]".into() })];
    let doc = with_blocks(&t, &b, &[], &tall_page(400.0));
    let pg = &doc.pages[0];
    assert_eq!(pg.strokes.len(), pg.stroke_chars.len());
    assert!(doc.block_errors.is_empty());
    // 2 outlines + 1 link + 1 arrowhead + 3 label glyphs.
    let drawn = tagged(&doc, 2);
    assert_eq!(drawn.len(), 7);
    let bottom = drawn.iter().map(|s| y_span(s).1).fold(f32::MIN, f32::max);
    assert!(drawn.iter().all(|s| y_span(s).0 >= 10.0), "below the first line");
    assert!(tagged(&doc, 4).iter().all(|s| y_span(s).0 > bottom), "text after it goes below");
    assert_eq!(doc.boxes.len(), 5);
}

#[test]
fn flowchart_scales_down_to_the_line_width() {
    let src = "graph LR\nA[XXXX]-->B[XXXX]-->C[XXXX]-->D[XXXX]".to_string();
    let doc = with_blocks(&format!("{OBJECT}"), &[(0, Block::Diagram { source: src })], &[], &tall_page(400.0));
    let right = tagged(&doc, 0).iter().map(|s| x_span(s).1).fold(f32::MIN, f32::max);
    assert!(right <= 100.0 + 1e-3, "{right}");
}

#[test]
fn flowchart_syntax_error_is_reported_not_drawn() {
    let doc = with_blocks(&format!("{OBJECT}"), &[(0, Block::Diagram { source: "pie\n\"a\": 1".into() })], &[], &tall_page(400.0));
    assert_eq!(doc.block_errors.len(), 1);
    assert_eq!(doc.stroke_count(), 0);
    assert_eq!(doc.boxes.len(), 1);
}
