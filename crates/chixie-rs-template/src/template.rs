//! Page templates: pre-printed paper the text is laid out on (ChiXie3 模板管理).
//!
//! - `Blank`: free lines every `line_height` (the default page).
//! - `Ruled`: letter paper with N rule lines per page (fixed, or a random count per page),
//!   optional dual-line band and equal columns.
//! - `Grid`: composition paper (作文格), one character per cell.
//! - `Table`: forms (表格模板); text fills the cells one segment each (see [`crate::table`]).
//!
//! Templates only position text; their ruling is drawn in the editor but never plotted.
//! Stored as `templates.toml` next to `settings.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use chixie_rs_core::layout::Ruling;

use crate::table::TableTemplate;
use chixie_rs_core::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    /// (width, height) mm.
    pub paper: (f32, f32),
    /// (left, top, right, bottom) mm: the writable area. For `Grid` the right/bottom margins
    /// follow from the grid size (`Template::grid_margins`).
    pub margins: (f32, f32, f32, f32),
    pub kind: TemplateKind,
    /// 标签 in the template gallery (全部模板 / 人物角色 / 教育学习 / …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// 我的收藏.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favourite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplateKind {
    Blank,
    Ruled {
        /// Rule lines per page; `rows_min < rows_max` draws a count per page (随机行数).
        rows_min: u32,
        rows_max: u32,
        /// Dual-line mode: upper band height (mm) above each writing line.
        dual_line: Option<f32>,
        /// Draw the dual-line partition dashed.
        dashed: bool,
        columns: u32,
        column_gap: f32,
        /// Per-column widths (mm) when 栏宽相等 is off; empty = equal columns.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        column_widths: Vec<f32>,
        /// Vertical divider between columns (分隔线).
        divider: bool,
        /// Dual-line mode: write into the upper band instead of below it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        write_upper: bool,
    },
    Grid {
        rows: u32,
        cols: u32,
        cell_w: f32,
        cell_h: f32,
        row_gap: f32,
        col_gap: f32,
    },
    Table(TableTemplate),
}

impl Template {
    pub fn blank(name: &str, paper: (f32, f32)) -> Self {
        Self { name: name.into(), paper, margins: (21.0, 20.0, 21.0, 20.0), kind: TemplateKind::Blank, tags: Vec::new(), favourite: false }
    }

    /// Ruled letter paper with a fixed line count.
    pub fn ruled(name: &str, paper: (f32, f32), margins: (f32, f32, f32, f32), rows: u32) -> Self {
        let kind = TemplateKind::Ruled {
            rows_min: rows,
            rows_max: rows,
            dual_line: None,
            dashed: true,
            columns: 1,
            column_gap: 6.0,
            column_widths: Vec::new(),
            divider: true,
            write_upper: false,
        };
        Self { name: name.into(), paper, margins, kind, tags: Vec::new(), favourite: false }
    }

    /// Composition grid placed `left`/`top` from the paper edge.
    #[allow(clippy::too_many_arguments)]
    pub fn grid(name: &str, paper: (f32, f32), left: f32, top: f32, rows: u32, cols: u32, cell: (f32, f32), gap: (f32, f32)) -> Self {
        let kind = TemplateKind::Grid { rows, cols, cell_w: cell.0, cell_h: cell.1, row_gap: gap.1, col_gap: gap.0 };
        let mut t = Self { name: name.into(), paper, margins: (left, top, 0.0, 0.0), kind, tags: Vec::new(), favourite: false };
        t.margins = t.grid_margins();
        t
    }

    /// An empty table form (one blank page; 插入表格 adds tables).
    pub fn table(name: &str, paper: (f32, f32)) -> Self {
        Self { name: name.into(), paper, margins: (0.0, 0.0, 0.0, 0.0), kind: TemplateKind::Table(TableTemplate::new()), tags: Vec::new(), favourite: false }
    }

    /// Margins for a grid: left/top as set, right/bottom = what the grid leaves of the paper.
    /// For a table form: the first page's tables are the frame (the pen starts at their corner).
    pub fn grid_margins(&self) -> (f32, f32, f32, f32) {
        let (l, t, ..) = self.margins;
        if let TemplateKind::Table(tt) = &self.kind {
            let Some((x0, y0, x1, y1)) = tt.frame() else { return (0.0, 0.0, 0.0, 0.0) };
            return (x0, y0, (self.paper.0 - x1).max(0.0), (self.paper.1 - y1).max(0.0));
        }
        match self.kind {
            TemplateKind::Grid { rows, cols, cell_w, cell_h, row_gap, col_gap } => {
                let w = cols as f32 * cell_w + cols.saturating_sub(1) as f32 * col_gap;
                let h = rows as f32 * cell_h + rows.saturating_sub(1) as f32 * row_gap;
                (l, t, (self.paper.0 - l - w).max(0.0), (self.paper.1 - t - h).max(0.0))
            }
            _ => self.margins,
        }
    }

    /// Layout ruling for this template.
    pub fn ruling(&self) -> Ruling {
        match &self.kind {
            TemplateKind::Blank => Ruling::Free,
            TemplateKind::Ruled { rows_min, rows_max, dual_line, dashed, columns, column_gap, column_widths, divider, write_upper } => Ruling::Rows {
                rows: ((*rows_min).max(1), (*rows_max).max(*rows_min).max(1)),
                columns: (*columns).max(1),
                column_gap: *column_gap,
                widths: column_widths.clone(),
                band: *dual_line,
                upper: *write_upper,
                dashed: *dashed,
                divider: *divider,
            },
            &TemplateKind::Grid { rows, cols, cell_w, cell_h, row_gap, col_gap } => {
                Ruling::Grid { rows: rows.max(1), cols: cols.max(1), cell: (cell_w, cell_h), gap: (col_gap, row_gap) }
            }
            TemplateKind::Table(tt) => {
                let (l, t, ..) = self.grid_margins();
                Ruling::Table { pages: tt.cell_boxes(l, t) }
            }
        }
    }

    /// Use this template's paper and margins for `page` and remember it as the active template.
    pub fn apply_to(&self, page: &mut chixie_rs_core::config::PageSettings) {
        page.paper = self.paper;
        page.margins = self.grid_margins();
        page.template = Some(self.name.clone());
    }

    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            TemplateKind::Blank => "空白",
            TemplateKind::Ruled { .. } => "信纸",
            TemplateKind::Grid { .. } => "作文格",
            TemplateKind::Table(_) => "表格",
        }
    }

    /// ChiXie3's word for this kind in its save messages (笔记 / 表格 / 作文).
    fn family(&self) -> &'static str {
        match self.kind {
            TemplateKind::Blank | TemplateKind::Ruled { .. } => "笔记",
            TemplateKind::Grid { .. } => "作文",
            TemplateKind::Table(_) => "表格",
        }
    }

    /// The checks ChiXie3 runs before saving a template (its messages, verbatim).
    pub fn validate(&self) -> std::result::Result<(), TemplateError> {
        if self.name.trim().is_empty() {
            return Err(TemplateError::EmptyName);
        }
        let (w, h) = self.paper;
        if !(w > 0.0 && h > 0.0) {
            return Err(TemplateError::PaperSize);
        }
        let (l, t, r, b) = self.margins;
        if [l, t, r, b].iter().any(|m| *m < 0.0 || !m.is_finite()) || l + r >= w || t + b >= h {
            return Err(TemplateError::BadInput);
        }
        match &self.kind {
            TemplateKind::Blank => {}
            TemplateKind::Ruled { rows_min, rows_max, column_widths, .. } => {
                if rows_min > rows_max {
                    return Err(TemplateError::RowsRange);
                }
                if *rows_min == 0 || column_widths.iter().any(|w| *w <= 0.0) {
                    return Err(TemplateError::BadInput);
                }
            }
            &TemplateKind::Grid { rows, cols, cell_w, cell_h, row_gap, col_gap } => {
                if rows == 0 || cols == 0 || cell_w <= 0.0 || cell_h <= 0.0 || row_gap < 0.0 || col_gap < 0.0 {
                    return Err(TemplateError::BadInput);
                }
                let grid_h = t + rows as f32 * cell_h + rows.saturating_sub(1) as f32 * row_gap;
                if grid_h > h + 1e-3 {
                    return Err(TemplateError::GridTooTall);
                }
                let grid_w = l + cols as f32 * cell_w + cols.saturating_sub(1) as f32 * col_gap;
                if grid_w > w + 1e-3 {
                    return Err(TemplateError::GridTooWide);
                }
            }
            TemplateKind::Table(tt) => {
                if tt.pages.is_empty() || tt.pages.iter().any(|p| p.tables.is_empty()) {
                    return Err(TemplateError::BlankTablePage);
                }
            }
        }
        Ok(())
    }
}

/// Why a template can't be saved. `Display` is ChiXie3's message.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TemplateError {
    #[error("模板名称不可为空")]
    EmptyName,
    /// Another template already has the name; the field is 笔记 / 表格 / 作文.
    #[error("{0}模板中已经存在该模板名，请修改模板名称")]
    Duplicate(&'static str),
    #[error("最小行数不能大于最大行数")]
    RowsRange,
    #[error("页面宽度高度必须大于0")]
    PaperSize,
    #[error("行数+行间距+上边距的数值超出页面高度，请检查")]
    GridTooTall,
    #[error("列数+列间距+左边距的数值超出页面宽度，请检查")]
    GridTooWide,
    #[error("数据输入异常")]
    BadInput,
    #[error("存在空白页面，无法新建表格模板")]
    BlankTablePage,
}

/// Paper sizes behind ChiXie3's 快捷尺寸 buttons (portrait, mm).
pub const A4: (f32, f32) = (210.0, 297.0);
pub const B5: (f32, f32) = (176.0, 250.0);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TemplateStore {
    #[serde(default, rename = "template")]
    pub templates: Vec<Template>,
}

impl TemplateStore {
    /// Built-in starting set (also what a missing file yields).
    pub fn builtins() -> Self {
        Self {
            templates: vec![
                Template::blank("空白 A4", (210.0, 297.0)),
                Template::ruled("信纸 22 行", (160.0, 219.8), (1.5, 1.0, 1.5, 1.0), 22),
                Template::grid("作文格 20×20", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9)),
            ],
        }
    }

    pub fn path_next_to(settings_path: &Path) -> PathBuf {
        settings_path.parent().unwrap_or_else(|| Path::new(".")).join("templates.toml")
    }

    /// Load; a missing file yields the built-ins.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|e| Error::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::builtins()),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic save (write a temp file, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// `base`, or `base (2)`, `base (3)`… whichever is free.
    pub fn unique_name(&self, base: &str) -> String {
        if self.get(base).is_none() {
            return base.to_string();
        }
        (2..).map(|n| format!("{base} ({n})")).find(|n| self.get(n).is_none()).expect("unbounded")
    }

    /// Add or replace by name; returns the index.
    pub fn upsert(&mut self, t: Template) -> usize {
        match self.templates.iter().position(|x| x.name == t.name) {
            Some(i) => {
                self.templates[i] = t;
                i
            }
            None => {
                self.templates.push(t);
                self.templates.len() - 1
            }
        }
    }

    /// `t` may be saved: valid, and its name not taken by another template (`editing` is the name
    /// it was opened under, so renaming to itself is fine).
    pub fn check(&self, t: &Template, editing: Option<&str>) -> std::result::Result<(), TemplateError> {
        t.validate()?;
        if Some(t.name.as_str()) != editing && self.get(&t.name).is_some() {
            return Err(TemplateError::Duplicate(t.family()));
        }
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Option<Template> {
        let i = self.templates.iter().position(|t| t.name == name)?;
        Some(self.templates.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use chixie_rs_core::font::{Font, Glyph};
    use chixie_rs_core::layout::{PageBox, PageParams, layout};

    /// 'x' is a 7×7 box glyph with advance 7.
    fn font() -> Font {
        let g = Glyph { advance: 7.0, strokes: vec![vec![(0.0, 0.0), (7.0, 0.0), (7.0, 7.0), (0.0, 7.0)]] };
        Font { glyphs: HashMap::from([('x', g)]) }
    }

    fn params(t: &Template) -> PageParams {
        let (l, top, r, b) = t.margins;
        PageParams {
            page: Some(PageBox { width: t.paper.0 - l - r, height: t.paper.1 - top - b }),
            ruling: t.ruling(),
            ..Default::default()
        }
    }

    #[test]
    fn ruled_rows_set_pitch_and_page_breaks() {
        // 100 mm writable height, 10 rows → 10 mm pitch; 12 lines → 2 pages.
        let t = Template::ruled("r", (120.0, 120.0), (10.0, 10.0, 10.0, 10.0), 10);
        let text = ["x"; 12].join("\n");
        let doc = layout(&text, &font(), &params(&t));
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.lines[0].top, 0.0);
        assert_eq!(doc.lines[1].top, -10.0);
        assert_eq!(doc.lines[10].page, 1);
        assert_eq!(doc.lines[10].top, 0.0);
        // Glyph bottom sits above the row's rule (y = -10) and inside the row.
        let ys: Vec<f32> = doc.pages[0].strokes[0].iter().map(|p| p.1).collect();
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        assert!(lo > -10.0 && hi < 0.0 && lo < -8.0, "glyph y range {lo}..{hi}");
        // Ruling: 11 rules on page 0 (top + each row bottom), none ever in strokes.
        assert_eq!(doc.pages[0].guides.len(), 11);
    }

    #[test]
    fn random_rows_are_per_page_and_deterministic() {
        let mut t = Template::ruled("r", (120.0, 220.0), (10.0, 10.0, 10.0, 10.0), 10);
        if let TemplateKind::Ruled { rows_min, rows_max, .. } = &mut t.kind {
            (*rows_min, *rows_max) = (18, 24);
        }
        let text = vec!["x"; 200].join("\n");
        let a = layout(&text, &font(), &params(&t));
        let b = layout(&text, &font(), &params(&t));
        assert_eq!(a, b);
        let counts: Vec<usize> = (0..a.pages.len()).map(|k| a.lines.iter().filter(|l| l.page == k).count()).collect();
        for c in &counts[..counts.len() - 1] {
            assert!((18..=24).contains(c), "rows {c}");
        }
        assert!(counts.windows(2).any(|w| w[0] != w[1]), "row counts should vary: {counts:?}");
    }

    #[test]
    fn columns_fill_top_to_bottom_then_right() {
        let mut t = Template::ruled("r", (120.0, 120.0), (10.0, 10.0, 10.0, 10.0), 5);
        if let TemplateKind::Ruled { columns, column_gap, .. } = &mut t.kind {
            (*columns, *column_gap) = (2, 10.0);
        }
        let text = ["x"; 7].join("\n");
        let doc = layout(&text, &font(), &params(&t));
        assert_eq!(doc.pages.len(), 1);
        // Column width (100 - 10) / 2 = 45; line 5 starts the second column.
        assert_eq!(doc.boxes[doc.lines[4].start].x0, 0.0);
        assert_eq!(doc.boxes[doc.lines[5].start].x0, 55.0);
        assert_eq!(doc.lines[5].top, 0.0);
    }

    #[test]
    fn grid_centres_one_char_per_cell_and_wraps() {
        let t = Template::grid("g", (100.0, 100.0), 10.0, 10.0, 3, 4, (10.0, 10.0), (0.0, 2.0));
        assert_eq!(t.margins, (10.0, 10.0, 50.0, 56.0)); // grid 40 × 34 mm
        let doc = layout("xxxxx x", &font(), &params(&t));
        // 7 chars, 4 per row → 2 rows; 5th char starts row 2.
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.boxes[4].x0, 0.0);
        assert_eq!(doc.lines[1].top, -12.0);
        // 'x' (7 mm) centred in a 10 mm cell: left edge 1.5 mm into the cell.
        let x0 = doc.pages[0].strokes[1].iter().map(|p| p.0).fold(f32::MAX, f32::min);
        assert!((x0 - 11.5).abs() < 1e-4, "x0 {x0}");
        // The space took a cell; 3×4 cells × 4 edges of guides.
        assert_eq!(doc.boxes[5].x0, 10.0);
        assert_eq!(doc.pages[0].guides.len(), 48);
    }

    fn fixture_font() -> Font {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../chixie-rs-core/tests/fixtures/font_fixture.json");
        Font::from_json(&std::fs::read(p).unwrap()).unwrap()
    }

    /// Ink top/bottom (mm, y up) of the strokes drawn for one glyph.
    fn ink_y(strokes: &[Vec<(f32, f32)>]) -> (f32, f32) {
        strokes.iter().flatten().fold((f32::MIN, f32::MAX), |(t, b), p| (t.max(p.1), b.min(p.1)))
    }

    /// ChiXie3 capture 2026-11-07, 我的笔记1 (A4, margins 20/30/20/20, 24 rows), 艾玛体 100,
    /// deformation off: 你 ink tops per row were −2.565, −22.424 (rows 1 and 3), bottoms −8.836,
    /// −28.777. ChiXie3 adds ≤ 0.3 mm vertical jitter plus a per-row spread (row residuals vs our
    /// mean model: +0.31, +0.09, −0.40 mm), so allow 0.6 mm.
    #[test]
    fn ruled_rows_match_chixie3_capture() {
        let t = Template::ruled("我的笔记1", (210.0, 297.0), (20.0, 30.0, 20.0, 20.0), 24);
        let doc = layout("你\n\n你", &fixture_font(), &params(&t));
        let (t1, b1) = ink_y(&doc.pages[0].strokes[..6]); // 你 = 6 strokes
        let (t3, b3) = ink_y(&doc.pages[0].strokes[6..]);
        for (ours, theirs) in [(t1, -2.565), (b1, -8.836), (t3, -22.424), (b3, -28.777)] {
            assert!((ours - theirs).abs() < 0.6, "ours {ours} vs captured {theirs}");
        }
    }

    /// ChiXie3 capture 2026-11-07, 作文格1 (8 mm cells, row gap 2.9): 你 in the 4th cell spans
    /// x 24.97..32.29, y −1.49..−7.85 (incl. ≤ 0.3 mm jitter): centred in its cell.
    #[test]
    fn grid_cells_match_chixie3_capture() {
        let t = Template::grid("作文格1", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9));
        let doc = layout("好好好你", &fixture_font(), &params(&t));
        let ni = &doc.pages[0].strokes[doc.pages[0].strokes.len() - 6..]; // 你 = 6 strokes
        let x0 = ni.iter().flatten().map(|p| p.0).fold(f32::MAX, f32::min);
        let (top, bottom) = ink_y(ni);
        for (ours, theirs) in [(x0, 24.97), (top, -1.485), (bottom, -7.851)] {
            assert!((ours - theirs).abs() < 0.7, "ours {ours} vs captured {theirs}");
        }
    }

    #[test]
    fn store_round_trips_and_names_are_unique() {
        let dir = std::env::temp_dir().join("chixie-rs-core-test-templates");
        let path = dir.join("templates.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(TemplateStore::load(&path).unwrap(), TemplateStore::builtins());
        let mut s = TemplateStore::builtins();
        let name = s.unique_name("信纸 22 行");
        assert_eq!(name, "信纸 22 行 (2)");
        s.upsert(Template::ruled(&name, (160.0, 220.0), (1.0, 1.0, 1.0, 1.0), 20));
        s.save(&path).unwrap();
        assert_eq!(TemplateStore::load(&path).unwrap(), s);
        assert!(s.remove(&name).is_some());
    }

    #[test]
    fn custom_column_widths_and_old_files_still_load() {
        let mut t = Template::ruled("r", (120.0, 120.0), (10.0, 10.0, 10.0, 10.0), 5);
        if let TemplateKind::Ruled { columns, column_gap, column_widths, .. } = &mut t.kind {
            (*columns, *column_gap, *column_widths) = (2, 10.0, vec![60.0, 30.0]);
        }
        let doc = layout(&["x"; 7].join("\n"), &font(), &params(&t));
        assert_eq!(doc.boxes[doc.lines[5].start].x0, 70.0); // 60 + gap 10
        // Pre-M7 files have no `column_widths` / `write_upper`.
        let old = "name = \"o\"\npaper = [160.0, 220.0]\nmargins = [1.0, 1.0, 1.0, 1.0]\n[kind]\ntype = \"ruled\"\nrows_min = 20\nrows_max = 20\ndashed = true\ncolumns = 1\ncolumn_gap = 6.0\ndivider = true\n";
        let t: Template = toml::from_str(old).unwrap();
        assert!(matches!(t.kind, TemplateKind::Ruled { ref column_widths, write_upper: false, .. } if column_widths.is_empty()));
    }

    #[test]
    fn upper_band_puts_text_above_the_partition() {
        let mut t = Template::ruled("r", (120.0, 120.0), (10.0, 10.0, 10.0, 10.0), 5);
        if let TemplateKind::Ruled { dual_line, .. } = &mut t.kind {
            *dual_line = Some(10.0);
        }
        let low = layout("x", &font(), &params(&t));
        if let TemplateKind::Ruled { write_upper, .. } = &mut t.kind {
            *write_upper = true;
        }
        let up = layout("x", &font(), &params(&t));
        let top = |d: &chixie_rs_core::layout::Document| ink_y(&d.pages[0].strokes).0;
        // Rows are 20 mm with a 10 mm band: lower text sits under it, upper text inside it.
        assert!(top(&low) <= -10.0 + 1e-3, "lower {}", top(&low));
        assert!(top(&up) > -10.0, "upper {}", top(&up));
    }

    #[test]
    fn table_forms_lay_out_from_their_frame_and_round_trip() {
        use crate::table::ColWidth;
        let mut t = Template::table("表", A4);
        assert_eq!(t.validate().unwrap_err().to_string(), "存在空白页面，无法新建表格模板");
        if let TemplateKind::Table(tt) = &mut t.kind {
            tt.insert_table(0, 2, 2, ColWidth::Fixed(40.0), A4.0, 20.0).unwrap();
        }
        assert_eq!(t.validate(), Ok(()));
        // The frame is the table: the pen starts at its corner.
        assert_eq!(t.grid_margins(), (20.0, 20.0, A4.0 - 100.0, A4.1 - 40.0));
        let b = chixie_rs_core::layout::CELL_BREAK;
        let doc = layout(&format!("x{b}x{b}x{b}x"), &font(), &params(&t));
        assert_eq!(doc.cells.len(), 4);
        assert_eq!(doc.cells[3].rect, (40.0, -10.0, 80.0, -20.0));
        let store = TemplateStore { templates: vec![t.clone()] };
        let text = toml::to_string_pretty(&store).unwrap();
        assert_eq!(toml::from_str::<TemplateStore>(&text).unwrap(), store);
    }

    /// ChiXie3 capture 2026-11-18 (captures/m7/table22.gcode): 插入表格 2×2 on A4 (x 21, y 30,
    /// cells 84 × 12 mm, 内部行数 1), 艾玛体 100, deformation off; cells 你 | 好 / 你好 | 好你好.
    /// Ink of each cell's first char (origin = the table's corner):
    /// 你 x 3.04, y −3.35..−9.78; 好 x 85.94, y −3.04..−8.68; 你 x 2.07, y −14.69..−21.76;
    /// 好 x 86.02, y −15.25..−21.10. ChiXie3 starts each row 0–2 mm in at random (首字不对齐, on in
    /// its config) and varies glyph shapes per layout, so: tops within 0.6 mm, and the left edge
    /// between our padding and padding + 2 mm.
    #[test]
    fn table_cells_match_chixie3_capture() {
        use crate::table::ColWidth;
        let mut t = Template::table("新建表格", A4);
        if let TemplateKind::Table(tt) = &mut t.kind {
            tt.insert_table(0, 2, 2, ColWidth::Fixed(84.0), A4.0, 21.0).unwrap();
            tt.pages[0].tables[0].rows.fill(12.0);
            tt.pages[0].tables[0].y = 30.0;
        }
        let b = chixie_rs_core::layout::CELL_BREAK;
        let doc = layout(&format!("你{b}好{b}你好{b}好你好"), &fixture_font(), &params(&t));
        let p = &doc.pages[0];
        let ink = |ch: usize| {
            let s: Vec<Vec<(f32, f32)>> = p.strokes.iter().zip(&p.stroke_chars).filter(|(_, k)| **k == ch).map(|(s, _)| s.clone()).collect();
            (s.iter().flatten().map(|q| q.0).fold(f32::MAX, f32::min), ink_y(&s).0)
        };
        // (char index, cell left, captured left, captured top)
        for (ch, cell_x, left, top) in [(0, 0.0, 3.04, -3.35), (2, 84.0, 85.94, -3.04), (4, 0.0, 2.07, -14.69), (7, 84.0, 86.02, -15.25)] {
            let (x0, t0) = ink(ch);
            assert!((t0 - top).abs() < 0.6, "char {ch}: top {t0} vs captured {top}");
            // Our ink starts at padding (1 mm) + the glyph's own bearing; ChiXie3 adds 0..2 mm.
            let bearing = x0 - (cell_x + 1.0);
            assert!(left >= x0 - 0.3 && left <= x0 + 2.0 + 0.3, "char {ch}: left {x0} (bearing {bearing}) vs captured {left}");
        }
    }

    #[test]
    fn validation_uses_chixie3_messages() {
        let ok = Template::ruled("n", A4, (20.0, 20.0, 20.0, 20.0), 20);
        assert_eq!(ok.validate(), Ok(()));
        let bad = |f: &dyn Fn(&mut Template)| {
            let mut t = ok.clone();
            f(&mut t);
            t.validate().unwrap_err().to_string()
        };
        assert_eq!(bad(&|t| t.name = " ".into()), "模板名称不可为空");
        assert_eq!(bad(&|t| t.paper = (0.0, 10.0)), "页面宽度高度必须大于0");
        assert_eq!(bad(&|t| t.margins = (150.0, 0.0, 100.0, 0.0)), "数据输入异常");
        assert_eq!(
            bad(&|t| if let TemplateKind::Ruled { rows_min, .. } = &mut t.kind {
                *rows_min = 30;
            }),
            "最小行数不能大于最大行数"
        );
        let grid = Template::grid("g", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9));
        assert_eq!(grid.validate(), Ok(()));
        let mut tall = grid.clone();
        tall.kind = TemplateKind::Grid { rows: 30, cols: 20, cell_w: 8.0, cell_h: 8.0, row_gap: 2.9, col_gap: 0.0 };
        assert_eq!(tall.validate().unwrap_err().to_string(), "行数+行间距+上边距的数值超出页面高度，请检查");
        let mut wide = grid.clone();
        wide.kind = TemplateKind::Grid { rows: 20, cols: 30, cell_w: 8.0, cell_h: 8.0, row_gap: 2.9, col_gap: 0.0 };
        assert_eq!(wide.validate().unwrap_err().to_string(), "列数+列间距+左边距的数值超出页面宽度，请检查");
        let store = TemplateStore::builtins();
        let dup = Template::ruled("信纸 22 行", A4, (20.0, 20.0, 20.0, 20.0), 20);
        assert_eq!(store.check(&dup, None).unwrap_err().to_string(), "笔记模板中已经存在该模板名，请修改模板名称");
        assert_eq!(store.check(&dup, Some("信纸 22 行")), Ok(()));
        let mut g2 = grid;
        g2.name = "作文格 20×20".into();
        assert_eq!(store.check(&g2, None).unwrap_err().to_string(), "作文模板中已经存在该模板名，请修改模板名称");
    }
}
