//! 导入 xlsx (also xls / ods through `calamine`): each sheet becomes a grid of cell texts, merged
//! cells expanded (every square of a merge reads the merge's value), and the grid fills a table
//! form page through its Excel range (see [`crate::table::TableTemplate::import_sheets`]).
//! Formulas give their cached results. Numbers print without trailing zeros (`calamine` does
//! not expose the cell's number format, so `0.00` shows `36.5`); dates print as ChiXie3's
//! `m"月"d"日"` (M月D日), the time part dropped.

use std::io::Cursor;

use calamine::{Data, Dimensions, Reader, Sheets, open_workbook_auto_from_rs};

/// A sheet as text: `rows[r][c]`, 0-based absolute positions (A1 = [0][0]).
pub type Sheet = Vec<Vec<String>>;

/// Every sheet of a workbook, in order.
pub fn read_sheets(bytes: &[u8]) -> Result<Vec<Sheet>, String> {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
    let names = wb.sheet_names().to_vec();
    let mut merges: Vec<(String, Dimensions)> = Vec::new();
    if let Sheets::Xlsx(x) = &mut wb {
        for name in &names {
            for d in x.merge_cells_by_sheet_name(name).map_err(|e| e.to_string())? {
                merges.push((name.clone(), d));
            }
        }
    }
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        let mut grid: Sheet = Vec::new();
        if let Some((r0, c0)) = range.start() {
            for (dr, row) in range.rows().enumerate() {
                for (dc, v) in row.iter().enumerate() {
                    let t = cell_text(v);
                    if !t.is_empty() {
                        put(&mut grid, r0 as usize + dr, c0 as usize + dc, t);
                    }
                }
            }
        }
        for (_, d) in merges.iter().filter(|m| m.0 == name) {
            let v = get(&grid, d.start.0 as usize, d.start.1 as usize).to_string();
            for r in d.start.0..=d.end.0 {
                for c in d.start.1..=d.end.1 {
                    put(&mut grid, r as usize, c as usize, v.clone());
                }
            }
        }
        out.push(grid);
    }
    Ok(out)
}

fn put(g: &mut Sheet, r: usize, c: usize, v: String) {
    if g.len() <= r {
        g.resize(r + 1, Vec::new());
    }
    if g[r].len() <= c {
        g[r].resize(c + 1, String::new());
    }
    g[r][c] = v;
}

/// Cell text at 0-based (row, col), "" when outside the sheet.
pub fn get(g: &Sheet, r: usize, c: usize) -> &str {
    g.get(r).and_then(|row| row.get(c)).map_or("", String::as_str)
}

fn cell_text(v: &Data) -> String {
    match v {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) => s.trim_end().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(d) if d.is_datetime() => month_day(d.as_f64()),
        Data::DateTime(d) => number(d.as_f64()),
        // "2026-11-18T08:00:00" → the date only.
        Data::DateTimeIso(s) => s.split('T').next().unwrap_or(s).to_string(),
        Data::DurationIso(s) => s.clone(),
    }
}

/// Shortest text for a number (15 significant digits, no trailing zeros).
fn number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    let s = format!("{f:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Excel serial day (1900 system) → M月D日.
fn month_day(serial: f64) -> String {
    // Days since 1970-01-01 (serial 25569), then Howard Hinnant's civil-from-days.
    let z = serial.floor() as i64 - 25569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{m}月{d}日")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_dates_print_like_chixie3() {
        assert_eq!(number(37.0), "37");
        assert_eq!(number(36.5), "36.5");
        assert_eq!(number(0.1 + 0.2), "0.3");
        // 2026-11-18 is serial 46344; 1900-03-01 is 61.
        assert_eq!(month_day(46344.75), "11月18日");
        assert_eq!(month_day(61.0), "3月1日");
    }

    /// Fixture made with openpyxl: sheet 体温 has B1:C1 merged ("姓名"), A2 = 36.5, A3 a date
    /// (2026-11-18), B2 = "张三", C3 = formula =A2+1 (cached 37.5); sheet 2 has A1 = 1.
    #[test]
    fn workbook_with_merges_reads_into_grids() {
        let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/merged.xlsx")).unwrap();
        let sheets = read_sheets(&bytes).unwrap();
        assert_eq!(sheets.len(), 2);
        let s = &sheets[0];
        assert_eq!((get(s, 0, 1), get(s, 0, 2)), ("姓名", "姓名"), "merge expanded");
        assert_eq!(get(s, 1, 0), "36.5");
        assert_eq!(get(s, 2, 0), "11月18日");
        assert_eq!(get(s, 1, 1), "张三");
        assert_eq!(get(s, 2, 2), "37.5");
        assert_eq!(get(&sheets[1], 0, 0), "1");
        assert_eq!(get(s, 40, 40), "");
    }
}
