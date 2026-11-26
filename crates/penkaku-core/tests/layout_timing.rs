//! Layout timing on a long document (M9). Not a pass/fail test: run with
//! `cargo test -p penkaku-core --release --test layout_timing -- --ignored --nocapture`
//! and compare the printed numbers before and after a change.

use std::time::Instant;

use penkaku_core::config::Settings;
use penkaku_core::font::Font;
use penkaku_core::layout::{self, Ruling};

const FIXTURE: &[u8] = include_bytes!("fixtures/font_fixture.json");

fn text(n: usize) -> String {
    // The fixture font has 你, 好, A and '.'; a paragraph break every 300 chars.
    let unit = ['你', '好', 'A', '你', '好', '.', ' '];
    (0..n).map(|i| if i % 300 == 299 { '\n' } else { unit[i % unit.len()] }).collect()
}

fn time(label: &str, runs: u32, mut f: impl FnMut() -> usize) {
    let mut pages = f();
    let t = Instant::now();
    for _ in 0..runs {
        pages = f();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
    println!("LAYOUT_TIMING {label:<28} {ms:8.2} ms  pages={pages}");
}

#[test]
#[ignore]
fn layout_timing() {
    let font = Font::from_json(FIXTURE).unwrap();
    let text = text(8000);
    let settings = Settings::default();
    let plain = settings.page_params_with(Ruling::Free);
    let mut styled = plain.clone();
    styled.style.enabled = true;
    styled.deformation.enabled = true;
    styled.deformation.base = true;
    styled.deformation.shape = true;
    let rows = settings.page_params_with(Ruling::Rows { rows: (22, 22), columns: 1, column_gap: 0.0, widths: vec![], band: None, upper: false, dashed: false, divider: false });
    let grid = settings.page_params_with(Ruling::Grid { rows: 20, cols: 20, cell: (8.0, 8.0), gap: (1.0, 2.0) });
    time("free, plain", 5, || layout::layout(&text, &font, &plain).pages.len());
    let mut hand = plain.clone();
    hand.style.enabled = true;
    time("free, handwriting only", 5, || layout::layout(&text, &font, &hand).pages.len());
    let mut deform = plain.clone();
    (deform.deformation.enabled, deform.deformation.base, deform.deformation.shape) = (true, true, true);
    time("free, deformation only", 5, || layout::layout(&text, &font, &deform).pages.len());
    time("free, handwriting+deform", 5, || layout::layout(&text, &font, &styled).pages.len());
    // The editor's case: the cache is warm from the previous keystroke.
    let cache = layout::DeformCache::new();
    let chars: Vec<char> = text.chars().collect();
    let ids: Vec<u64> = (0..chars.len() as u64).collect();
    let flow = layout::Flow { chars: &chars, ids: &ids, deform_cache: Some(&cache), ..Default::default() };
    time("free, styled, warm cache", 5, || layout::layout_flow(&flow, &font, &styled).pages.len());
    time("信纸 22 rows", 5, || layout::layout(&text, &font, &rows).pages.len());
    time("作文格 20x20", 5, || layout::layout(&text, &font, &grid).pages.len());
}
