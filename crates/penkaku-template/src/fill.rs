//! Filling a table form's cells: data-source values (刷新数据) and imported rows (批量导入). The
//! document is one flow; cell k of page copy j is the flow's segment `start(j) + k` (segments are
//! split at `CELL_BREAK`). Fills come back as edits on that flow so they can be one undo step.

use std::ops::Range;

use penkaku_core::layout::CELL_BREAK;

use crate::table::{DataSource, TableTemplate};

/// One edit on the flow: replace `range` with `text` (an empty range at the end appends).
pub type Edit = (Range<usize>, String);

/// splitmix64: a well-mixed u64 from a key.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl DataSource {
    /// The cell's value on page copy `page`, cell `cell` (writing order). Random values are fixed
    /// by `seed` (换一组 = another seed). `None`: nothing to fill (no source, or import-bound).
    pub fn value(&self, seed: u64, page: usize, cell: usize) -> Option<String> {
        let r = mix(seed ^ mix((page as u64) << 32 | cell as u64));
        match self {
            DataSource::None | DataSource::FromImport { .. } => None,
            DataSource::Fixed { value } => Some(value.clone()),
            DataSource::RandomNumber { min, max, decimals } => {
                let (lo, hi) = (min.min(*max), min.max(*max));
                let u = (r >> 11) as f64 / (1u64 << 53) as f64;
                Some(format!("{:.*}", *decimals as usize, lo + u * (hi - lo)))
            }
            DataSource::RandomChoice { choices } => {
                let c: Vec<&String> = choices.iter().filter(|s| !s.trim().is_empty()).collect();
                (!c.is_empty()).then(|| c[(r % c.len() as u64) as usize].clone())
            }
        }
    }

    /// ChiXie3's check before saving a 随机值 source.
    pub fn check(&self) -> Result<(), &'static str> {
        match self {
            DataSource::RandomNumber { min, max, .. } if min > max => Err("最小值不能大于最大值"),
            _ => Ok(()),
        }
    }
}

/// Segment ranges of `chars` (split at `CELL_BREAK`; always at least one).
pub fn segments(chars: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c == CELL_BREAK {
            out.push(start..i);
            start = i + 1;
        }
    }
    out.push(start..chars.len());
    out
}

impl TableTemplate {
    /// Cells of each page, for page copy `j`.
    fn page_cells(&self, j: usize) -> Vec<(usize, usize)> {
        self.cells_in_order(j % self.pages.len().max(1))
    }

    /// First segment index of page copy `j`.
    pub fn first_segment(&self, j: usize) -> usize {
        (0..j).map(|k| self.page_cells(k).len()).sum()
    }

    /// Page copy holding segment `seg`.
    pub fn page_of_segment(&self, seg: usize) -> usize {
        let mut j = 0;
        while self.first_segment(j + 1) <= seg && !self.page_cells(j).is_empty() {
            j += 1;
        }
        j
    }

    /// Edits that make page copies `pages` hold `values[(copy, cell)]`, adding cell breaks at the
    /// end of the text when the pages don't exist yet. Applied last-first, they keep each other's
    /// ranges valid.
    fn set_cells(&self, chars: &[char], values: Vec<(usize, String)>) -> Vec<Edit> {
        let segs = segments(chars);
        let mut edits: Vec<Edit> = Vec::new();
        let mut tail: Vec<String> = Vec::new();
        for (seg, text) in values {
            match segs.get(seg) {
                Some(r) => {
                    if chars[r.clone()].iter().collect::<String>() != text {
                        edits.push((r.clone(), text));
                    }
                }
                None => {
                    let k = seg - segs.len();
                    if tail.len() <= k {
                        tail.resize(k + 1, String::new());
                    }
                    tail[k] = text;
                }
            }
        }
        if !tail.is_empty() {
            // First among edits at the same place (the stable sort keeps it so): applied
            // last-first, the last segment's own edit then lands before it.
            let b = CELL_BREAK.to_string();
            edits.insert(0, (chars.len()..chars.len(), format!("{b}{}", tail.join(&b))));
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        edits
    }

    /// 刷新数据: every data-source cell of page copies `pages` gets its value.
    pub fn refresh(&self, chars: &[char], pages: Range<usize>, seed: u64) -> Vec<Edit> {
        let mut values = Vec::new();
        for j in pages {
            let first = self.first_segment(j);
            let k = j % self.pages.len().max(1);
            for (n, (t, c)) in self.page_cells(j).into_iter().enumerate() {
                if let Some(v) = self.pages[k].tables[t].cells[c].data.value(seed, j, n) {
                    values.push((first + n, v));
                }
            }
        }
        self.set_cells(chars, values)
    }

    /// Page copies the text fills (at least one).
    pub fn page_count(&self, chars: &[char]) -> usize {
        let n = segments(chars).len();
        let mut j = 0;
        while self.first_segment(j + 1) < n && !self.page_cells(j).is_empty() {
            j += 1;
        }
        j + 1
    }

    /// 导入 xlsx: sheet j fills page copy j. A single-page template makes one page per sheet; a
    /// multi-page one needs one sheet per page (ChiXie3's check). Each page reads from its Excel
    /// range (A1 when unset), table cells mapped by grid position (tables stacked downwards, a
    /// merged cell reads its top-left square); `指定单元格` cells read their own row / column.
    /// Cells with `接受导入` off or another data source are left alone.
    pub fn import_sheets(&self, chars: &[char], sheets: &[crate::xlsx::Sheet]) -> Result<Vec<Edit>, &'static str> {
        if self.pages.len() > 1 && sheets.len() != self.pages.len() {
            return Err("多页模板页数和所导入的表格sheet页数不一致");
        }
        let mut values = Vec::new();
        for (j, sheet) in sheets.iter().enumerate() {
            let page = &self.pages[j % self.pages.len()];
            let ((c0, r0), last) = page.excel_range.map_or(((1, 1), None), |r| (r.first, Some(r.last)));
            let first = self.first_segment(j);
            for (n, (t, c)) in self.page_cells(j).into_iter().enumerate() {
                let cell = &page.tables[t].cells[c];
                let below: u32 = page.tables[..t].iter().map(|x| x.rows.len() as u32).sum();
                let at = match cell.data {
                    DataSource::FromImport { row, col } => Some((row.max(1) - 1, col.max(1) - 1)),
                    DataSource::None if cell.accept_import => {
                        let (r, col) = (r0 - 1 + below + cell.span.r0, c0 - 1 + cell.span.c0);
                        last.is_none_or(|(lc, lr)| r < lr && col < lc).then_some((r, col))
                    }
                    _ => None,
                };
                if let Some((r, col)) = at {
                    values.push((first + n, crate::xlsx::get(sheet, r as usize, col as usize).to_string()));
                }
            }
        }
        Ok(self.set_cells(chars, values))
    }

    /// 批量名单建页 (single-page templates): one page per line of `names`, the line written into
    /// cell `target` (writing order); the other cells get their data-source values. The text is
    /// replaced from page 1 on.
    pub fn batch_pages(&self, chars: &[char], names: &[String], target: usize, seed: u64) -> Result<Vec<Edit>, &'static str> {
        if self.pages.len() != 1 {
            return Err("暂时只支持单页模板批量建页");
        }
        let cells = self.page_cells(0);
        if target >= cells.len() {
            return Err("请先选中目标单元格");
        }
        let names: Vec<&String> = names.iter().filter(|s| !s.trim().is_empty()).collect();
        let mut segs: Vec<String> = Vec::new();
        for (j, name) in names.iter().enumerate() {
            for (n, &(t, c)) in cells.iter().enumerate() {
                segs.push(if n == target { name.trim().to_string() } else { self.pages[0].tables[t].cells[c].data.value(seed, j, n).unwrap_or_default() });
            }
        }
        // Drop empty cells at the very end (they need no break).
        while segs.last().is_some_and(|s| s.is_empty()) && segs.len() > 1 {
            segs.pop();
        }
        Ok(vec![(0..chars.len(), segs.join(&CELL_BREAK.to_string()))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{ColWidth, TableTemplate};

    /// One page, a 2×2 table: cells 0..4 in writing order.
    fn form() -> TableTemplate {
        let mut tt = TableTemplate::new();
        tt.insert_table(0, 2, 2, ColWidth::Fixed(30.0), 210.0, 10.0).unwrap();
        let cells = &mut tt.pages[0].tables[0].cells;
        cells[1].data = DataSource::Fixed { value: "晴".into() };
        cells[3].data = DataSource::RandomNumber { min: 36.0, max: 37.5, decimals: 1 };
        tt
    }

    fn apply(chars: &[char], edits: &[Edit]) -> String {
        let mut s: Vec<char> = chars.to_vec();
        for (r, t) in edits {
            s.splice(r.clone(), t.chars());
        }
        s.into_iter().collect()
    }

    #[test]
    fn values_are_deterministic_per_seed_and_page() {
        let d = DataSource::RandomNumber { min: 36.0, max: 37.5, decimals: 1 };
        let v = d.value(7, 0, 3).unwrap();
        assert_eq!(v, d.value(7, 0, 3).unwrap());
        let n: f64 = v.parse().unwrap();
        assert!((36.0..=37.5).contains(&n) && v.split('.').nth(1).unwrap().len() == 1);
        let pages: std::collections::HashSet<String> = (0..20).map(|j| d.value(7, j, 3).unwrap()).collect();
        assert!(pages.len() > 1, "pages differ");
        assert_ne!((0..20).map(|j| d.value(7, j, 3).unwrap()).collect::<Vec<_>>(), (0..20).map(|j| d.value(8, j, 3).unwrap()).collect::<Vec<_>>());
        let pick = DataSource::RandomChoice { choices: vec!["甲".into(), " ".into(), "乙".into()] };
        assert!(["甲", "乙"].contains(&pick.value(1, 0, 0).unwrap().as_str()));
        assert_eq!(DataSource::RandomNumber { min: 2.0, max: 1.0, decimals: 0 }.check(), Err("最小值不能大于最大值"));
    }

    #[test]
    fn refresh_fills_existing_and_missing_cells() {
        let b = CELL_BREAK;
        let tt = form();
        let text: Vec<char> = format!("张三{b}雨").chars().collect();
        let out = apply(&text, &tt.refresh(&text, 0..2, 5));
        let segs: Vec<&str> = out.split(b).collect();
        assert_eq!(segs.len(), 8, "two pages of four cells");
        assert_eq!((segs[0], segs[1], segs[2], segs[5]), ("张三", "晴", "", "晴"));
        assert!(segs[3].parse::<f64>().is_ok() && segs[7].parse::<f64>().is_ok());
        assert_eq!(tt.page_count(&out.chars().collect::<Vec<_>>()), 2);
        assert_eq!(tt.page_of_segment(5), 1);
    }

    #[test]
    fn xlsx_with_merges_maps_onto_the_form() {
        let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/merged.xlsx")).unwrap();
        let sheets = crate::xlsx::read_sheets(&bytes).unwrap();
        // 3×2 table, top row merged; read from B1; the bottom-right cell is bound to A2.
        let mut tt = TableTemplate::new();
        tt.insert_table(0, 3, 2, ColWidth::Fixed(30.0), 210.0, 10.0).unwrap();
        let t = &mut tt.pages[0].tables[0];
        t.merge(&[0, 1]).unwrap();
        let bound = t.cells.iter().position(|c| c.span.r0 == 2 && c.span.c0 == 1).unwrap();
        t.cells[bound].data = DataSource::FromImport { row: 2, col: 1 };
        let skip = t.cells.iter().position(|c| c.span.r0 == 1 && c.span.c0 == 1).unwrap();
        t.cells[skip].accept_import = false;
        tt.pages[0].excel_range = Some(crate::table::CellRange::from_fields("B", "1", "C", "3").unwrap());
        let out = apply(&[], &tt.import_sheets(&[], &sheets).unwrap());
        let segs: Vec<&str> = out.split(CELL_BREAK).collect();
        // Page 1: merged top row = B1 (姓名); row 2: B2 (张三) and the skipped cell; row 3: B3
        // (empty) and the cell bound to A2 (36.5).
        assert_eq!(&segs[..5], &["姓名", "张三", "", "", "36.5"]);
        assert_eq!(tt.page_count(&out.chars().collect::<Vec<_>>()), 2, "one page per sheet");
        let mut two = tt.clone();
        two.add_page();
        two.add_page();
        assert_eq!(two.import_sheets(&[], &sheets), Err("多页模板页数和所导入的表格sheet页数不一致"));
    }

    #[test]
    fn a_thirty_line_list_makes_thirty_pages() {
        let tt = form();
        let names: Vec<String> = (1..=30).map(|i| format!("学生{i}")).collect();
        let text: Vec<char> = "旧".chars().collect();
        let out = apply(&text, &tt.batch_pages(&text, &names, 0, 1).unwrap());
        let chars: Vec<char> = out.chars().collect();
        assert_eq!(tt.page_count(&chars), 30);
        let segs = segments(&chars);
        assert_eq!(chars[segs[4].clone()].iter().collect::<String>(), "学生2");
        assert_eq!(chars[segs[5].clone()].iter().collect::<String>(), "晴");
        let mut two = form();
        two.add_page();
        assert_eq!(two.batch_pages(&text, &names, 0, 1), Err("暂时只支持单页模板批量建页"));
        assert_eq!(tt.batch_pages(&text, &names, 9, 1), Err("请先选中目标单元格"));
    }
}
