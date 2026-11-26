//! M7b done-when: the detector recovers every cell, merged cells included, on 10 forms built
//! here (5 vector PDFs, 3 photos / screenshots, 2 scans). Real forms dropped into
//! `tests/forms-real/` (git-ignored) are checked too: `name.cells.json` holds the expected cell
//! spans of each table (`[[[r0, c0, r1, c1], …], …]`); without it, at least one table must be found.

use penkaku_import::synth::{self, Form, Shot, Style};
use penkaku_import::{DetectedTable, detect};

/// Same cells; column / row proportions within 1.5 % of the table (scale-free, so it also holds
/// after unwarping); for vector PDFs also the absolute edges within 0.5 mm.
fn check(found: &DetectedTable, f: &Form, absolute: bool, name: &str) {
    assert_eq!(found.cells, f.cells, "{name}: cells");
    let (fw, fh) = (f.xs[f.xs.len() - 1] - f.xs[0], f.ys[f.ys.len() - 1] - f.ys[0]);
    for (k, c) in found.cols.iter().enumerate() {
        let want = (f.xs[k + 1] - f.xs[k]) / fw;
        assert!((c / found.width() - want).abs() < 0.015, "{name}: col {k} {} vs {want}", c / found.width());
        if absolute {
            assert!((c - (f.xs[k + 1] - f.xs[k])).abs() < 0.5, "{name}: col {k} {c} mm");
        }
    }
    for (k, r) in found.rows.iter().enumerate() {
        let want = (f.ys[k + 1] - f.ys[k]) / fh;
        assert!((r / found.height() - want).abs() < 0.015, "{name}: row {k} {} vs {want}", r / found.height());
    }
    if absolute {
        assert!((found.x - f.xs[0]).abs() < 0.5 && (found.y - f.ys[0]).abs() < 0.5, "{name}: corner");
    }
}

fn png(img: &image::GrayImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn one(bytes: &[u8], f: &Form, absolute: bool, name: &str) {
    let pages = detect(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(pages.len(), 1, "{name}");
    assert_eq!(pages[0].tables.len(), 1, "{name}: tables");
    check(&pages[0].tables[0], f, absolute, name);
}

#[test]
fn ten_forms() {
    // 5 vector PDFs.
    one(&synth::pdf(&[(Form::merged_header(), Style::Lines, false)]), &Form::merged_header(), true, "pdf lines");
    one(&synth::pdf(&[(Form::mixed(), Style::Rects, false)]), &Form::mixed(), true, "pdf rects");
    one(&synth::pdf(&[(Form::dense(), Style::Bars, false)]), &Form::dense(), true, "pdf bars");
    one(&synth::pdf(&[(Form::mixed(), Style::Lines, true)]), &Form::mixed(), true, "pdf transformed");
    let (a, b) = (Form::merged_header(), Form::grid(30.0, 150.0, &[40.0; 3], &[12.0; 2]).merge((0, 0, 2, 1)));
    let pages = detect(&synth::pdf_forms(&[a.clone(), b.clone()], Style::Lines)).unwrap();
    assert_eq!(pages[0].tables.len(), 2, "two tables");
    check(&pages[0].tables[0], &a, true, "two tables: first");
    check(&pages[0].tables[1], &b, true, "two tables: second");
    // 3 photos / screenshots.
    one(&png(&synth::render(&Form::merged_header(), &Shot::screenshot())), &Form::merged_header(), false, "screenshot");
    one(&png(&synth::render(&Form::mixed(), &Shot::photo(4.0, 0.06, 2))), &Form::mixed(), false, "photo tilted");
    let strong = Shot { light: 0.45, ..Shot::photo(-7.0, 0.14, 3) };
    one(&png(&synth::render(&Form::dense(), &strong)), &Form::dense(), false, "photo trapezoid");
    // 2 scans.
    one(&png(&synth::render(&Form::mixed(), &Shot::scan(1.2, 4))), &Form::mixed(), false, "scan png");
    let scanned = synth::render(&Form::merged_header(), &Shot::scan(-0.8, 5));
    one(&synth::pdf_with_image(&scanned), &Form::merged_header(), false, "scan pdf");
}

#[test]
fn left_padding_finds_the_first_ink() {
    // A 100 × 40 px cell with its border, a speck, and text starting 23 px in.
    let mut img = image::GrayImage::from_pixel(100, 40, image::Luma([240]));
    for x in 0..100 {
        img.put_pixel(x, 0, image::Luma([10]));
        img.put_pixel(x, 39, image::Luma([10]));
    }
    for y in 0..40 {
        img.put_pixel(0, y, image::Luma([10]));
    }
    img.put_pixel(12, 20, image::Luma([10]));
    for y in 15..25 {
        img.put_pixel(23, y, image::Luma([30]));
    }
    assert_eq!(penkaku_import::left_padding(&img, (0, 0, 100, 40), 3), Some(23));
    assert_eq!(penkaku_import::left_padding(&image::GrayImage::from_pixel(50, 20, image::Luma([250])), (0, 0, 50, 20), 2), None);
}

#[test]
fn real_forms_dropped_in() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/forms-real");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let ext = p.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("pdf" | "png" | "jpg" | "jpeg")) {
            continue;
        }
        let pages = detect(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let tables: Vec<&DetectedTable> = pages.iter().flat_map(|pg| &pg.tables).collect();
        assert!(!tables.is_empty(), "{}: no table found", p.display());
        let sidecar = p.with_extension("cells.json");
        if let Ok(text) = std::fs::read_to_string(&sidecar) {
            let want = parse_cells(&text);
            let got: Vec<Vec<(u32, u32, u32, u32)>> = tables.iter().map(|t| t.cells.clone()).collect();
            assert_eq!(got, want, "{}", p.display());
        }
    }
}

/// `[[[r0, c0, r1, c1], …], …]` without a JSON dependency.
fn parse_cells(text: &str) -> Vec<Vec<(u32, u32, u32, u32)>> {
    let nums: Vec<i64> = text.split(|c: char| !c.is_ascii_digit() && c != '[' && c != ']').flat_map(|s| s.split(['[', ']'])).filter_map(|s| s.trim().parse().ok()).collect();
    // Tables are separated by "]]," at depth 1; count cells per table from the bracket structure.
    let mut tables = Vec::new();
    let (mut depth, mut cur, mut i) = (0, Vec::new(), 0);
    for ch in text.chars() {
        match ch {
            '[' => depth += 1,
            ']' => {
                if depth == 3 {
                    cur.push((nums[i] as u32, nums[i + 1] as u32, nums[i + 2] as u32, nums[i + 3] as u32));
                    i += 4;
                } else if depth == 2 {
                    tables.push(std::mem::take(&mut cur));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    tables
}
