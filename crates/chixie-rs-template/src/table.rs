//! Table form templates (ChiXie3 表格模板): pages of tables made of cells. A table is a grid of
//! column widths and row heights; its cells partition that grid (a merged cell spans several grid
//! squares). Text fills the cells in reading order, one `CELL_BREAK`-separated segment each.

use serde::{Deserialize, Serialize};

use chixie_rs_core::layout::{Align, CellBox, Overflow, VAlign};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableTemplate {
    /// At least one page; the document repeats them in order.
    pub pages: Vec<TablePage>,
    /// 同型标记 records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub same_type: Vec<SameType>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TablePage {
    #[serde(default)]
    pub tables: Vec<Table>,
    /// Excel导入配置: the sheet range this page reads on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excel_range: Option<CellRange>,
    /// The scanned / photographed form this page was made from, shown under the table in the
    /// editor (never written) and used by 内边距识别.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
}

/// A page background image: a PNG in the template assets folder, placed on the paper (mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Background {
    /// File name inside the template assets folder.
    pub file: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 透明度 (0 = hidden, 1 = opaque).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

fn default_opacity() -> f32 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// Top-left corner, mm from the paper's top-left.
    pub x: f32,
    pub y: f32,
    /// 位置锁定.
    #[serde(default)]
    pub locked: bool,
    /// Grid column widths and row heights (mm).
    pub cols: Vec<f32>,
    pub rows: Vec<f32>,
    /// Partition of the grid, in the order they were made (writing order is by position).
    pub cells: Vec<Cell>,
}

/// Grid squares `r0..r1` × `c0..c1` (end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub r0: u32,
    pub c0: u32,
    pub r1: u32,
    pub c1: u32,
}

impl Span {
    pub fn one(r: u32, c: u32) -> Self {
        Self { r0: r, c0: c, r1: r + 1, c1: c + 1 }
    }
    fn area(&self) -> u32 {
        (self.r1 - self.r0) * (self.c1 - self.c0)
    }
    fn overlaps(&self, o: &Span) -> bool {
        self.r0 < o.r1 && o.r0 < self.r1 && self.c0 < o.c1 && o.c0 < self.c1
    }
    fn union(&self, o: &Span) -> Span {
        Span { r0: self.r0.min(o.r0), c0: self.c0.min(o.c0), r1: self.r1.max(o.r1), c1: self.c1.max(o.c1) }
    }
}

/// A parent cell (母单元格) and its attributes (表格单元格属性).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cell {
    pub span: Span,
    /// 内部行数: writing rows inside the cell.
    pub rows: u32,
    /// 排版内边距 (left, top, right, bottom) mm.
    pub padding: (f32, f32, f32, f32),
    /// 接受导入: filled by xlsx / batch import.
    pub accept_import: bool,
    /// 提示标识: shown in the editor, never written.
    pub hint: String,
    /// Character size relative to the document's (默认字号).
    pub size: f32,
    pub align: Align,
    pub valign: VAlign,
    pub overflow: Overflow,
    pub data: DataSource,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            span: Span::one(0, 0),
            rows: 1,
            padding: (1.0, 0.0, 1.0, 0.0),
            accept_import: true,
            hint: String::new(),
            size: 1.0,
            align: Align::Left,
            valign: VAlign::Top,
            overflow: Overflow::default(),
            data: DataSource::None,
        }
    }
}

/// 数据源配置: where a cell's text comes from when pages are made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataSource {
    /// 无数据源: typed or imported.
    #[default]
    None,
    /// 默认值.
    Fixed { value: String },
    /// 随机值 › 数值范围内随机.
    RandomNumber { min: f64, max: f64, decimals: u32 },
    /// 随机值 › 给定内容内随机: one of the lines.
    RandomChoice { choices: Vec<String> },
    /// 指定单元格: row / column of an imported sheet (1-based).
    FromImport { row: u32, col: u32 },
}

/// 同型标记: cells that hold the same kind of content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SameType {
    pub name: String,
    pub cells: Vec<CellRef>,
}

/// One cell of a template: page, table, cell index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellRef {
    pub page: usize,
    pub table: usize,
    pub cell: usize,
}

/// An Excel range like `B2:F20`; columns and rows 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRange {
    pub first: (u32, u32),
    pub last: (u32, u32),
}

/// `A` → 1, `Z` → 26, `AA` → 27 (ChiXie3 `excel_column_to_number`).
pub fn column_number(letters: &str) -> Option<u32> {
    if letters.is_empty() {
        return None;
    }
    letters.chars().try_fold(0u32, |n, c| c.is_ascii_alphabetic().then(|| n * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1)))
}

pub fn column_letters(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

impl CellRange {
    /// From ChiXie3's four fields (左上列, 左上行, 右下列, 右下行), with its messages.
    pub fn from_fields(c0: &str, r0: &str, c1: &str, r1: &str) -> Result<Self, TableError> {
        if [c0, r0, c1, r1].iter().any(|s| s.trim().is_empty()) {
            return Err(TableError::RangeIncomplete);
        }
        let row = |s: &str| s.trim().parse::<u32>().ok().filter(|r| *r > 0).ok_or(TableError::RangeRow);
        let col = |s: &str| column_number(s.trim()).ok_or(TableError::RangeColumn);
        let (a, b) = ((col(c0)?, row(r0)?), (col(c1)?, row(r1)?));
        Ok(Self { first: (a.0.min(b.0), a.1.min(b.1)), last: (a.0.max(b.0), a.1.max(b.1)) })
    }
}

impl std::fmt::Display for CellRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}:{}{}", column_letters(self.first.0), self.first.1, column_letters(self.last.0), self.last.1)
    }
}

/// Table edits that ChiXie3 refuses, with its messages where it has one.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TableError {
    #[error("列宽*列数超出页面宽度(还需保留最少2mm边距)，请重新设置")]
    TooWide,
    #[error("请先选中表格单元格")]
    NoSelection,
    #[error("只能合并组成矩形的相邻单元格")]
    NotRectangle,
    #[error("当前已经是第一页，无法上移")]
    FirstPage,
    #[error("当前已经是最后一页，无法下移")]
    LastPage,
    #[error("请至少保留一个页面")]
    LastOne,
    #[error("行号输入框中只能输入数字")]
    RangeRow,
    #[error("列号输入框中只能输入字母")]
    RangeColumn,
    #[error("如需保存请填写所有所有输入框数据")]
    RangeIncomplete,
}

/// 插入表格 › 列宽选择.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColWidth {
    /// 固定列宽 (mm).
    Fixed(f32),
    /// 自动列宽: the page width (minus margins) shared equally.
    Auto,
}

/// Default row height for new tables (mm).
pub const ROW_HEIGHT: f32 = 10.0;
/// ChiXie3 keeps at least this much paper to the right of a table.
const EDGE: f32 = 2.0;

impl Table {
    /// `rows` × `cols` single cells at (x, y).
    pub fn grid(x: f32, y: f32, rows: u32, cols: u32, col_w: f32, row_h: f32) -> Self {
        let cells = (0..rows).flat_map(|r| (0..cols).map(move |c| Cell { span: Span::one(r, c), ..Default::default() })).collect();
        Self { x, y, locked: false, cols: vec![col_w; cols as usize], rows: vec![row_h; rows as usize], cells }
    }

    pub fn width(&self) -> f32 {
        self.cols.iter().sum()
    }
    pub fn height(&self) -> f32 {
        self.rows.iter().sum()
    }

    /// A cell's box (x, y, w, h) in mm from the paper's top-left.
    pub fn rect(&self, s: Span) -> (f32, f32, f32, f32) {
        let x0 = self.x + self.cols[..s.c0 as usize].iter().sum::<f32>();
        let y0 = self.y + self.rows[..s.r0 as usize].iter().sum::<f32>();
        let w = self.cols[s.c0 as usize..s.c1 as usize].iter().sum();
        let h = self.rows[s.r0 as usize..s.r1 as usize].iter().sum();
        (x0, y0, w, h)
    }

    /// Cell indices in writing order: top to bottom, then left to right.
    pub fn reading_order(&self) -> Vec<usize> {
        let mut ix: Vec<usize> = (0..self.cells.len()).collect();
        ix.sort_by_key(|&i| (self.cells[i].span.r0, self.cells[i].span.c0));
        ix
    }

    /// 合并单元格: the selected cells must tile a rectangle. The merged cell keeps the first
    /// selected cell's attributes; returns its index.
    pub fn merge(&mut self, sel: &[usize]) -> Result<usize, TableError> {
        let mut sel: Vec<usize> = sel.iter().copied().filter(|&i| i < self.cells.len()).collect();
        sel.sort_unstable();
        sel.dedup();
        let Some(&first) = sel.first() else { return Err(TableError::NoSelection) };
        let bb = sel.iter().fold(self.cells[first].span, |b, &i| b.union(&self.cells[i].span));
        let area: u32 = sel.iter().map(|&i| self.cells[i].span.area()).sum();
        let intruder = (0..self.cells.len()).any(|i| !sel.contains(&i) && self.cells[i].span.overlaps(&bb));
        if sel.len() < 2 || area != bb.area() || intruder {
            return Err(TableError::NotRectangle);
        }
        let keep = self.cells[first].clone();
        let mut k = 0;
        self.cells.retain(|_| {
            k += 1;
            !sel.contains(&(k - 1))
        });
        self.cells.push(Cell { span: bb, ..keep });
        Ok(self.cells.len() - 1)
    }

    /// 拆分单元格: back to single grid squares (ChiXie3 splits completely, not one merge back).
    pub fn split(&mut self, i: usize) {
        let Some(c) = self.cells.get(i).cloned() else { return };
        if c.span.area() <= 1 {
            return;
        }
        self.cells.remove(i);
        for r in c.span.r0..c.span.r1 {
            for col in c.span.c0..c.span.c1 {
                self.cells.push(Cell { span: Span::one(r, col), ..c.clone() });
            }
        }
    }

    /// Move a grid line: column `c` (or row `r`) becomes `mm` wide (tall), at least 2 mm.
    pub fn set_col_width(&mut self, c: usize, mm: f32) {
        if let Some(w) = self.cols.get_mut(c) {
            *w = mm.max(2.0);
        }
    }
    pub fn set_row_height(&mut self, r: usize, mm: f32) {
        if let Some(h) = self.rows.get_mut(r) {
            *h = mm.max(2.0);
        }
    }
}

impl TableTemplate {
    pub fn new() -> Self {
        Self { pages: vec![TablePage::default()], same_type: Vec::new() }
    }

    /// 插入表格 on `page`: below the tables already there, at the left margin.
    pub fn insert_table(&mut self, page: usize, rows: u32, cols: u32, width: ColWidth, paper_w: f32, left: f32) -> Result<(), TableError> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let col_w = match width {
            ColWidth::Fixed(w) => w,
            ColWidth::Auto => ((paper_w - 2.0 * left.max(EDGE)) / cols as f32).floor().max(2.0),
        };
        if left + col_w * cols as f32 > paper_w - EDGE {
            return Err(TableError::TooWide);
        }
        let k = page.min(self.pages.len() - 1);
        let p = &mut self.pages[k];
        let y = p.tables.iter().map(|t| t.y + t.height() + 5.0).fold(left, f32::max);
        p.tables.push(Table::grid(left, y, rows, cols, col_w, ROW_HEIGHT));
        Ok(())
    }

    /// 增加空白页面.
    pub fn add_page(&mut self) {
        self.pages.push(TablePage::default());
    }

    /// 复制模板页面: a copy goes last.
    pub fn duplicate_page(&mut self, k: usize) {
        if let Some(p) = self.pages.get(k).cloned() {
            self.pages.push(p);
        }
    }

    /// 删除选中页面 (one must stay). Same-type marks follow the pages.
    pub fn remove_page(&mut self, k: usize) -> Result<(), TableError> {
        if self.pages.len() <= 1 {
            return Err(TableError::LastOne);
        }
        self.pages.remove(k.min(self.pages.len() - 1));
        for st in &mut self.same_type {
            st.cells.retain(|c| c.page != k);
            st.cells.iter_mut().filter(|c| c.page > k).for_each(|c| c.page -= 1);
        }
        self.same_type.retain(|s| s.cells.len() >= 2);
        Ok(())
    }

    /// 上移 / 下移选中页面.
    pub fn move_page(&mut self, k: usize, up: bool) -> Result<usize, TableError> {
        let to = match (up, k) {
            (true, 0) => return Err(TableError::FirstPage),
            (true, _) => k - 1,
            (false, _) if k + 1 >= self.pages.len() => return Err(TableError::LastPage),
            (false, _) => k + 1,
        };
        self.pages.swap(k, to);
        for c in self.same_type.iter_mut().flat_map(|s| s.cells.iter_mut()) {
            if c.page == k {
                c.page = to;
            } else if c.page == to {
                c.page = k;
            }
        }
        Ok(to)
    }

    /// Every cell of `page` in writing order (tables in order), as (table, cell) indices.
    pub fn cells_in_order(&self, page: usize) -> Vec<(usize, usize)> {
        let Some(p) = self.pages.get(page) else { return Vec::new() };
        p.tables.iter().enumerate().flat_map(|(t, tb)| tb.reading_order().into_iter().map(move |c| (t, c))).collect()
    }

    /// Bounding box (x0, y0, x1, y1) of the first page's tables: the writing frame.
    pub fn frame(&self) -> Option<(f32, f32, f32, f32)> {
        let ts = &self.pages.first()?.tables;
        ts.iter().map(|t| (t.x, t.y, t.x + t.width(), t.y + t.height())).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// Layout cells of every page, relative to the frame's top-left at (`left`, `top`) mm.
    pub fn cell_boxes(&self, left: f32, top: f32) -> Vec<Vec<CellBox>> {
        (0..self.pages.len())
            .map(|k| {
                self.cells_in_order(k)
                    .into_iter()
                    .map(|(t, c)| {
                        let tb = &self.pages[k].tables[t];
                        let cell = &tb.cells[c];
                        let (x, y, w, h) = tb.rect(cell.span);
                        CellBox {
                            x: x - left,
                            y: y - top,
                            w,
                            h,
                            rows: cell.rows.max(1),
                            padding: cell.padding,
                            align: cell.align,
                            valign: cell.valign,
                            scale: cell.size,
                            overflow: cell.overflow,
                        }
                    })
                    .collect()
            })
            .collect()
    }
}

impl Default for TableTemplate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(t: &Table) -> Vec<(u32, u32)> {
        t.reading_order().iter().map(|&i| (t.cells[i].span.r0, t.cells[i].span.c0)).collect()
    }

    #[test]
    fn merge_needs_a_rectangle_and_split_undoes_it_fully() {
        let mut t = Table::grid(0.0, 0.0, 3, 3, 10.0, 10.0);
        // Cells 0,1,3,4 = the top-left 2×2.
        let m = t.merge(&[0, 1, 3, 4]).unwrap();
        assert_eq!(t.cells[m].span, Span { r0: 0, c0: 0, r1: 2, c1: 2 });
        assert_eq!(t.cells.len(), 6);
        assert_eq!(t.rect(t.cells[m].span), (0.0, 0.0, 20.0, 20.0));
        assert_eq!(order(&t), vec![(0, 0), (0, 2), (1, 2), (2, 0), (2, 1), (2, 2)]);
        // An L shape, or one cell, is refused.
        let mut u = Table::grid(0.0, 0.0, 2, 2, 10.0, 10.0);
        assert_eq!(u.merge(&[0, 1, 2]), Err(TableError::NotRectangle));
        assert_eq!(u.merge(&[0]), Err(TableError::NotRectangle));
        assert_eq!(u.merge(&[]), Err(TableError::NoSelection));
        t.split(m);
        assert_eq!(t.cells.len(), 9);
        assert_eq!(order(&t).len(), 9);
    }

    #[test]
    fn insert_checks_the_width_and_stacks_tables() {
        let mut tt = TableTemplate::new();
        assert_eq!(tt.insert_table(0, 2, 10, ColWidth::Fixed(20.0), 210.0, 10.0), Err(TableError::TooWide));
        tt.insert_table(0, 2, 2, ColWidth::Fixed(30.0), 210.0, 10.0).unwrap();
        tt.insert_table(0, 1, 4, ColWidth::Auto, 210.0, 10.0).unwrap();
        let p = &tt.pages[0];
        assert_eq!(p.tables[1].y, 10.0 + 20.0 + 5.0);
        assert!(p.tables[1].width() <= 210.0 - 10.0 - 2.0);
        assert_eq!(tt.frame(), Some((10.0, 10.0, 10.0 + p.tables[1].width(), 35.0 + 10.0)));
        // Writing order: table 1's four cells, then table 2's.
        assert_eq!(tt.cells_in_order(0).len(), 8);
        let boxes = tt.cell_boxes(10.0, 10.0);
        assert_eq!((boxes[0][0].x, boxes[0][0].y), (0.0, 0.0));
        assert_eq!((boxes[0][3].x, boxes[0][3].y), (30.0, 10.0));
    }

    #[test]
    fn pages_keep_one_and_move_with_their_marks() {
        let mut tt = TableTemplate::new();
        assert_eq!(tt.remove_page(0), Err(TableError::LastOne));
        tt.add_page();
        tt.duplicate_page(0);
        assert_eq!(tt.pages.len(), 3);
        tt.same_type.push(SameType { name: "姓名".into(), cells: vec![CellRef { page: 0, table: 0, cell: 0 }, CellRef { page: 2, table: 0, cell: 0 }] });
        assert_eq!(tt.move_page(0, true), Err(TableError::FirstPage));
        assert_eq!(tt.move_page(2, false), Err(TableError::LastPage));
        assert_eq!(tt.move_page(0, false), Ok(1));
        assert_eq!(tt.same_type[0].cells[0].page, 1);
        tt.remove_page(2).unwrap();
        assert!(tt.same_type.is_empty(), "a record needs two cells");
    }

    #[test]
    fn excel_ranges_parse_like_chixie3() {
        assert_eq!(column_number("A"), Some(1));
        assert_eq!(column_number("aa"), Some(27));
        assert_eq!(column_letters(28), "AB");
        let r = CellRange::from_fields("F", "20", "b", "2").unwrap();
        assert_eq!(r.to_string(), "B2:F20");
        assert_eq!(CellRange::from_fields("1", "2", "B", "3").unwrap_err().to_string(), "列号输入框中只能输入字母");
        assert_eq!(CellRange::from_fields("A", "x", "B", "3").unwrap_err().to_string(), "行号输入框中只能输入数字");
        assert_eq!(CellRange::from_fields("A", "", "B", "3").unwrap_err().to_string(), "如需保存请填写所有所有输入框数据");
    }
}
