//! Table templates as XLSX, so a form can be drawn or adjusted in Excel / WPS:
//! - export: one worksheet per page, column widths and row heights at the template's size,
//!   merged cells merged, thin borders on every cell, hint labels as cell text;
//! - import: each sheet's used range becomes a page (tables split at empty rows) with the sheet's
//!   column widths, row heights and merged cells; cell text becomes the hint label.
//!
//! `calamine` has no column widths / row heights, so import reads the sheet XML itself.

use std::io::{Cursor, Read as _};

use quick_xml::Reader;
use quick_xml::events::Event;
use rust_xlsxwriter::{Format, FormatBorder, Workbook};
use zip::ZipArchive;

use crate::table::{Cell, Span, Table, TablePage, TableTemplate};

const PX_PER_MM: f64 = 96.0 / 25.4;
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Where an imported table's corner goes on the paper (mm).
const PLACE_MM: f32 = 20.0;

/// The table template as an XLSX workbook. Tables on one page are stacked with an empty row
/// between them; they share the page's column widths (the first table's).
pub fn export(tt: &TableTemplate) -> Result<Vec<u8>, String> {
    let mut wb = Workbook::new();
    let cell = Format::new().set_border(FormatBorder::Thin).set_text_wrap();
    for (k, page) in tt.pages.iter().enumerate() {
        let ws = wb.add_worksheet();
        ws.set_name(format!("第{}页", k + 1)).map_err(|e| e.to_string())?;
        let mut cols_set = 0usize;
        let mut row0 = 0u32;
        for t in &page.tables {
            for (c, w) in t.cols.iter().enumerate().skip(cols_set) {
                ws.set_column_width_pixels(c as u16, (*w as f64 * PX_PER_MM).round() as u32).map_err(|e| e.to_string())?;
            }
            cols_set = cols_set.max(t.cols.len());
            for (r, h) in t.rows.iter().enumerate() {
                ws.set_row_height(row0 + r as u32, (*h as f64 * PT_PER_MM * 100.0).round() / 100.0).map_err(|e| e.to_string())?;
            }
            for c in &t.cells {
                let s = c.span;
                let (r0, c0) = (row0 + s.r0, s.c0 as u16);
                if s.r1 - s.r0 > 1 || s.c1 - s.c0 > 1 {
                    ws.merge_range(r0, c0, row0 + s.r1 - 1, (s.c1 - 1) as u16, &c.hint, &cell).map_err(|e| e.to_string())?;
                } else if c.hint.is_empty() {
                    ws.write_blank(r0, c0, &cell).map_err(|e| e.to_string())?;
                } else {
                    ws.write_string_with_format(r0, c0, c.hint.clone(), &cell).map_err(|e| e.to_string())?;
                }
            }
            row0 += t.rows.len() as u32 + 1;
        }
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}

/// `B12` → (row 11, col 1), 0-based.
fn cell_ref(s: &str) -> Option<(u32, u32)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let col = crate::table::column_number(&s[..split])?;
    let row: u32 = s[split..].parse().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// Excel's column width (characters) → pixels (Calibri 11, 96 dpi).
fn width_px(w: f64) -> f64 {
    if w < 1.0 { (w * 12.0 + 0.5).floor() } else { ((256.0 * w + (128.0f64 / 7.0).floor()) / 256.0 * 7.0).floor() }
}

#[derive(Default)]
struct Sheet {
    /// Column ranges (0-based, inclusive) and their width in characters.
    cols: Vec<(u32, u32, f64)>,
    /// `defaultColWidth` when given; otherwise Excel's default column is 64 px.
    default_width: Option<f64>,
    heights: Vec<(u32, f64)>,
    default_height: f64,
    /// Cells present (row, col) and their text.
    cells: Vec<((u32, u32), String)>,
    merges: Vec<((u32, u32), (u32, u32))>,
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == key).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn parse_sheet(xml: &str, shared: &[String]) -> Result<Sheet, String> {
    let mut s = Sheet { default_height: 15.0, ..Default::default() };
    let mut r = Reader::from_str(xml);
    // Current cell: position, type, and the text read so far.
    let mut cur: Option<((u32, u32), String)> = None;
    let mut ty = String::new();
    let (mut in_v, mut in_t) = (false, false);
    loop {
        let ev = r.read_event().map_err(|e| e.to_string())?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"sheetFormatPr" => {
                    if let Some(w) = attr(e, b"defaultColWidth").and_then(|v| v.parse().ok()) {
                        s.default_width = Some(w);
                    }
                    if let Some(h) = attr(e, b"defaultRowHeight").and_then(|v| v.parse().ok()) {
                        s.default_height = h;
                    }
                }
                b"col" => {
                    let (a, b) = (attr(e, b"min").and_then(|v| v.parse::<u32>().ok()), attr(e, b"max").and_then(|v| v.parse::<u32>().ok()));
                    if let (Some(a), Some(b), Some(w)) = (a, b, attr(e, b"width").and_then(|v| v.parse::<f64>().ok())) {
                        s.cols.push((a.saturating_sub(1), b.saturating_sub(1), w));
                    }
                }
                b"row" => {
                    if let (Some(n), Some(h)) = (attr(e, b"r").and_then(|v| v.parse::<u32>().ok()), attr(e, b"ht").and_then(|v| v.parse::<f64>().ok())) {
                        s.heights.push((n.saturating_sub(1), h));
                    }
                }
                b"c" => {
                    if let Some(p) = attr(e, b"r").as_deref().and_then(cell_ref) {
                        ty = attr(e, b"t").unwrap_or_default();
                        if matches!(ev, Event::Empty(_)) {
                            s.cells.push((p, String::new()));
                        } else {
                            cur = Some((p, String::new()));
                        }
                    }
                }
                b"v" => in_v = true,
                b"t" => in_t = true,
                b"mergeCell" => {
                    if let Some(rf) = attr(e, b"ref")
                        && let Some((a, b)) = rf.split_once(':')
                        && let (Some(a), Some(b)) = (cell_ref(a), cell_ref(b))
                    {
                        s.merges.push((a, b));
                    }
                }
                _ => {}
            },
            Event::Text(t) if in_v || in_t => {
                if let Some(c) = cur.as_mut() {
                    c.1 += &t.decode().map_err(|e| e.to_string())?;
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"v" => in_v = false,
                b"t" => in_t = false,
                b"c" => {
                    if let Some((p, mut text)) = cur.take() {
                        if ty == "s" {
                            text = text.trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default();
                        }
                        s.cells.push((p, text));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(s)
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut r = Reader::from_str(xml);
    let (mut out, mut cur, mut in_t, mut in_si) = (Vec::new(), String::new(), false, false);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"si" => {
                in_si = true;
                cur.clear();
            }
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"si" => {
                in_si = false;
                out.push(std::mem::take(&mut cur));
            }
            Ok(Event::Text(t)) if in_t && in_si => cur += &t.decode().unwrap_or_default(),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

impl Sheet {
    fn col_mm(&self, c: u32) -> f32 {
        let px = match self.cols.iter().find(|(a, b, _)| c >= *a && c <= *b) {
            Some(x) => width_px(x.2),
            None => self.default_width.map_or(64.0, width_px),
        };
        (px / PX_PER_MM) as f32
    }
    fn row_mm(&self, r: u32) -> f32 {
        let h = self.heights.iter().find(|x| x.0 == r).map_or(self.default_height, |x| x.1);
        (h / PT_PER_MM) as f32
    }

    /// The used range split into tables at empty rows: (first row, last row, first col, last col).
    fn blocks(&self) -> Vec<(u32, u32, u32, u32)> {
        let mut rows: Vec<u32> = self.cells.iter().map(|c| c.0.0).collect();
        for &((r0, _), (r1, _)) in &self.merges {
            rows.extend(r0..=r1);
        }
        rows.sort_unstable();
        rows.dedup();
        let mut out = Vec::new();
        let mut i = 0;
        while i < rows.len() {
            let mut j = i;
            while j + 1 < rows.len() && rows[j + 1] == rows[j] + 1 {
                j += 1;
            }
            let (ra, rb) = (rows[i], rows[j]);
            let inside = |r: u32| r >= ra && r <= rb;
            let mut cols: Vec<u32> = self.cells.iter().filter(|c| inside(c.0.0)).map(|c| c.0.1).collect();
            for &((r0, c0), (r1, c1)) in &self.merges {
                if inside(r0) || inside(r1) {
                    cols.extend([c0, c1]);
                }
            }
            if let (Some(&ca), Some(&cb)) = (cols.iter().min(), cols.iter().max()) {
                out.push((ra, rb, ca, cb));
            }
            i = j + 1;
        }
        out
    }

    fn table(&self, (ra, rb, ca, cb): (u32, u32, u32, u32), y: f32) -> Table {
        let cols: Vec<f32> = (ca..=cb).map(|c| self.col_mm(c)).collect();
        let rows: Vec<f32> = (ra..=rb).map(|r| self.row_mm(r)).collect();
        let text = |r: u32, c: u32| self.cells.iter().find(|x| x.0 == (r, c)).map(|x| x.1.trim().to_string()).unwrap_or_default();
        let mut covered = vec![false; rows.len() * cols.len()];
        let mut cells = Vec::new();
        for &((r0, c0), (r1, c1)) in self.merges.iter().filter(|m| m.0.0 >= ra && m.1.0 <= rb && m.0.1 >= ca && m.1.1 <= cb) {
            let span = Span { r0: r0 - ra, c0: c0 - ca, r1: r1 - ra + 1, c1: c1 - ca + 1 };
            for r in span.r0..span.r1 {
                for c in span.c0..span.c1 {
                    covered[(r as usize) * cols.len() + c as usize] = true;
                }
            }
            cells.push(Cell { span, hint: text(r0, c0), ..Default::default() });
        }
        for r in 0..rows.len() as u32 {
            for c in 0..cols.len() as u32 {
                if !covered[r as usize * cols.len() + c as usize] {
                    cells.push(Cell { span: Span::one(r, c), hint: text(ra + r, ca + c), ..Default::default() });
                }
            }
        }
        Table { x: PLACE_MM, y, locked: false, cols, rows, cells }
    }
}

/// Read an XLSX workbook as a table template: one page per non-empty sheet.
pub fn import(bytes: &[u8]) -> Result<TableTemplate, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut read = |name: &str| -> Option<String> {
        let mut s = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    };
    let shared = read("xl/sharedStrings.xml").map(|x| shared_strings(&x)).unwrap_or_default();
    let mut names: Vec<(u32, String)> = Vec::new();
    for i in 1..=255 {
        let n = format!("xl/worksheets/sheet{i}.xml");
        if let Some(x) = read(&n) {
            names.push((i, x));
        }
    }
    let mut pages = Vec::new();
    for (_, xml) in names {
        let s = parse_sheet(&xml, &shared)?;
        let mut y = PLACE_MM;
        let mut tables = Vec::new();
        for b in s.blocks() {
            let t = s.table(b, y);
            y += t.height() + 5.0;
            tables.push(t);
        }
        if !tables.is_empty() {
            pages.push(TablePage { tables, ..Default::default() });
        }
    }
    if pages.is_empty() {
        return Err("表格文件里没有找到表格".into());
    }
    Ok(TableTemplate { pages, same_type: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::ColWidth;

    fn form() -> TableTemplate {
        let mut tt = TableTemplate::new();
        tt.insert_table(0, 3, 3, ColWidth::Fixed(30.0), 210.0, 20.0).unwrap();
        {
            let t = &mut tt.pages[0].tables[0];
            t.cols = vec![25.0, 40.0, 55.5];
            t.rows = vec![12.0, 8.5, 20.0];
            t.merge(&[0, 1, 2]).unwrap();
            let m = t.cells.len() - 1;
            t.cells[m].hint = "体温记录".into();
            t.cells[0].hint = "姓名".into();
        }
        tt.insert_table(0, 2, 2, ColWidth::Fixed(30.0), 210.0, 20.0).unwrap();
        tt.add_page();
        tt.insert_table(1, 2, 4, ColWidth::Fixed(20.0), 210.0, 20.0).unwrap();
        tt
    }

    fn spans(t: &Table) -> Vec<(u32, u32, u32, u32)> {
        let mut v: Vec<_> = t.cells.iter().map(|c| (c.span.r0, c.span.c0, c.span.r1, c.span.c1)).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn export_then_import_keeps_grid_merges_sizes_and_hints() {
        let tt = form();
        let back = import(&export(&tt).unwrap()).unwrap();
        assert_eq!(back.pages.len(), 2);
        assert_eq!(back.pages[0].tables.len(), 2, "two tables split at the empty row");
        for (a, b) in tt.pages.iter().flat_map(|p| &p.tables).zip(back.pages.iter().flat_map(|p| &p.tables)) {
            assert_eq!(spans(a), spans(b));
            for (x, y) in a.rows.iter().zip(&b.rows) {
                assert!((x - y).abs() < 0.5, "row {x} vs {y}");
            }
        }
        // Column widths: the page's first table sets them.
        let (a, b) = (&tt.pages[0].tables[0], &back.pages[0].tables[0]);
        for (x, y) in a.cols.iter().zip(&b.cols) {
            assert!((x - y).abs() < 0.5, "col {x} vs {y}");
        }
        let hints: Vec<&str> = b.cells.iter().map(|c| c.hint.as_str()).filter(|h| !h.is_empty()).collect();
        assert!(hints.contains(&"体温记录") && hints.contains(&"姓名"), "{hints:?}");
    }

    #[test]
    fn a_sheet_from_another_tool_imports() {
        // openpyxl: B1:C1 merged, values in A2..C3, default widths.
        let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/merged.xlsx")).unwrap();
        let tt = import(&bytes).unwrap();
        let t = &tt.pages[0].tables[0];
        assert_eq!((t.rows.len(), t.cols.len()), (3, 3));
        assert!(spans(t).contains(&(0, 1, 1, 3)), "B1:C1 merged: {:?}", spans(t));
        assert!((t.cols[0] - 64.0 / PX_PER_MM as f32).abs() < 0.5, "default width {}", t.cols[0]);
        assert!(import(b"not a zip").is_err());
    }
}
