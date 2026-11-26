//! Laid-out page → the page as it is written: row order (路径规划), 180° turn (页面旋转) and the
//! pen's start point (书写原点). Coordinates stay layout mm (origin at the frame's top-left, y up);
//! only the emitter maps them to machine axes.

use penkaku_core::config::Settings;
use penkaku_core::layout::{Document, Page};
use penkaku_core::profile::{BLACK, Origin, PageRotation, PathPlan, Rgb};

/// Paper and margins (mm: left, top, right, bottom), as in `PageSettings`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheet {
    pub paper: (f32, f32),
    pub margins: (f32, f32, f32, f32),
}

impl Sheet {
    /// Writable frame size (paper minus margins).
    pub fn frame(&self) -> (f32, f32) {
        let (l, t, r, b) = self.margins;
        (self.paper.0 - l - r, self.paper.1 - t - b)
    }

    /// The start point in layout mm (frame top-left = (0, 0), y up).
    pub fn origin_point(&self, o: Origin) -> (f32, f32) {
        let (l, t, ..) = self.margins;
        match o {
            Origin::Paper => (-l, t),
            Origin::Frame => (0.0, 0.0),
            Origin::Custom { x, y } => (x - l, t - y),
        }
    }

    /// The 书写原点 for a marker dropped at layout point `(x, y)`: the paper or frame corner when
    /// within `snap` mm of it, else a custom point rounded to whole mm (kept on the paper).
    pub fn snap_origin(&self, (x, y): (f32, f32), snap: f32) -> Origin {
        let near = |(a, b): (f32, f32)| (x - a).hypot(y - b) <= snap;
        if near(self.origin_point(Origin::Frame)) {
            return Origin::Frame;
        }
        if near(self.origin_point(Origin::Paper)) {
            return Origin::Paper;
        }
        let (l, t, ..) = self.margins;
        Origin::Custom { x: (x + l).round().clamp(0.0, self.paper.0), y: (t - y).round().clamp(0.0, self.paper.1) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputOptions {
    pub path_plan: PathPlan,
    pub origin: Origin,
    pub rotation: PageRotation,
    pub sheet: Sheet,
}

impl OutputOptions {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            path_plan: s.device.profile.path_plan,
            origin: s.page.origin,
            rotation: s.page.rotation,
            sheet: Sheet { paper: s.page.paper, margins: s.page.margins },
        }
    }
}

/// Page `k` of `doc` as written. With the defaults (之字型, 框线起点, 不旋转) this is a clone, so
/// the G-code stays byte-identical to the layout's.
pub fn plan(doc: &Document, k: usize, o: &OutputOptions) -> Page {
    let mut page = doc.pages[k].clone();
    let turn = o.rotation.turns(k);
    let (ox, oy) = o.sheet.origin_point(o.origin);
    let (w, h) = o.sheet.frame();
    match o.path_plan {
        PathPlan::Raster => {}
        // ChiXie3 reverses a row's text, not a grid's cells: 弓字型 leaves 作文格 pages as they
        // are (captured 2026-11-17, docs/protocol.md).
        PathPlan::Serpentine if doc.grid => {}
        PathPlan::Serpentine => order_rows(doc, &mut page, (0.0, 0.0), |row, _, _, _| row % 2 == 1),
        PathPlan::Nearest => {
            // The pen starts at the origin; in layout mm that is the origin point, turned with
            // the page (the 180° turn is its own inverse).
            let start = if turn { (w - ox, -h - oy) } else { (ox, oy) };
            let d = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
            order_rows(doc, &mut page, start, |_, pen, fwd, back| d(pen, back) < d(pen, fwd));
        }
    }
    if turn || (ox, oy) != (0.0, 0.0) {
        // 180° about the frame centre (w/2, -h/2): the turned page fills the same frame.
        let f = |(x, y): (f32, f32)| {
            let (x, y) = if turn { (w - x, -h - y) } else { (x, y) };
            (x - ox, y - oy)
        };
        for s in &mut page.strokes {
            s.iter_mut().for_each(|p| *p = f(*p));
        }
        for g in &mut page.guides {
            (g.a, g.b) = (f(g.a), f(g.b));
        }
    }
    page
}

/// A page as written, with the pen colour runs of 多色绘制.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Written {
    pub page: Page,
    /// (stroke index, colour) where each colour run starts, in writing order. Empty when
    /// 多色绘制 is off (one pen, no changes).
    pub pens: Vec<(usize, Rgb)>,
    /// 字符轻重 of each stroke, in steps (see [`Written::with_press`]). Empty = all standard.
    pub press: Vec<f32>,
}

impl Written {
    /// Each stroke takes its char's pressure, `press_of[char]` (missing = standard). Stays empty
    /// when every stroke is standard, so the G-code is unchanged.
    pub fn with_press(mut self, press_of: &[f32]) -> Self {
        let v: Vec<f32> = self.page.stroke_chars.iter().map(|&c| press_of.get(c).copied().unwrap_or(0.0)).collect();
        self.press = if v.iter().any(|&p| p != 0.0) { v } else { Vec::new() };
        self
    }
}

/// Page `k` as written. `colours[c]` is character `c`'s pen colour (`None` = black). With
/// `multicolour`, strokes are grouped by colour in order of first appearance, keeping writing
/// order inside a colour, so each pen is changed once per page.
pub fn write(doc: &Document, k: usize, colours: &[Option<Rgb>], multicolour: bool, o: &OutputOptions) -> Written {
    let mut page = plan(doc, k, o);
    if !multicolour || page.stroke_chars.len() != page.strokes.len() {
        return Written { page, pens: Vec::new(), press: Vec::new() };
    }
    let colour_of = |c: usize| colours.get(c).copied().flatten().unwrap_or(BLACK);
    let mut seen: Vec<Rgb> = Vec::new();
    for &c in &page.stroke_chars {
        let col = colour_of(c);
        if !seen.contains(&col) {
            seen.push(col);
        }
    }
    let rank = |c: usize| seen.iter().position(|&s| s == colour_of(c)).unwrap_or(0);
    let mut order: Vec<usize> = (0..page.strokes.len()).collect();
    order.sort_by_key(|&i| rank(page.stroke_chars[i]));
    reorder(&mut page, &order);
    let mut pens: Vec<(usize, Rgb)> = Vec::new();
    for (i, &c) in page.stroke_chars.iter().enumerate() {
        let col = colour_of(c);
        if pens.last().is_none_or(|p| p.1 != col) {
            pens.push((i, col));
        }
    }
    Written { page, pens, press: Vec::new() }
}

/// Every page of `doc` as written (see [`write`]).
pub fn plan_all(doc: &Document, colours: &[Option<Rgb>], multicolour: bool, o: &OutputOptions) -> Vec<Written> {
    (0..doc.pages.len()).map(|k| write(doc, k, colours, multicolour, o)).collect()
}

/// Put the strokes (and their chars) in `order`.
fn reorder(page: &mut Page, order: &[usize]) {
    let mut slots: Vec<Option<_>> = std::mem::take(&mut page.strokes).into_iter().map(Some).collect();
    page.strokes = order.iter().map(|&i| slots[i].take().unwrap_or_default()).collect();
    page.stroke_chars = order.iter().map(|&i| page.stroke_chars[i]).collect();
}

/// Rows of the page (by line, top to bottom; only rows that carry ink, so blank lines don't
/// count) written in turn, each left to right unless `reverse(row, pen, fwd, back)` says right to
/// left. `pen` is where the previous row ended (`start` before the first); `fwd` / `back` are the
/// row's first point written either way. Each character's strokes keep their order.
fn order_rows(doc: &Document, page: &mut Page, start: (f32, f32), mut reverse: impl FnMut(usize, (f32, f32), (f32, f32), (f32, f32)) -> bool) {
    if page.stroke_chars.len() != page.strokes.len() {
        return;
    }
    let line_of = |c: usize| doc.boxes.get(c).map_or(0, |b| b.line);
    let mut order: Vec<usize> = (0..page.strokes.len()).collect();
    // Stable: a character's strokes stay in writing order.
    order.sort_by_key(|&i| (line_of(page.stroke_chars[i]), page.stroke_chars[i]));
    let first = |ix: &[usize]| ix.iter().find_map(|&i| page.strokes[i].first().copied());
    let last = |ix: &[usize]| ix.iter().rev().find_map(|&i| page.strokes[i].last().copied());
    let mut out = Vec::with_capacity(order.len());
    let mut pen = start;
    for (row, ix) in order.chunk_by(|&a, &b| line_of(page.stroke_chars[a]) == line_of(page.stroke_chars[b])).enumerate() {
        // The row's characters, each a run of its strokes.
        let chars: Vec<&[usize]> = ix.chunk_by(|&a, &b| page.stroke_chars[a] == page.stroke_chars[b]).collect();
        let fwd = first(ix).unwrap_or(pen);
        let back = chars.iter().rev().find_map(|c| first(c)).unwrap_or(pen);
        let at = out.len();
        if reverse(row, pen, fwd, back) {
            chars.iter().rev().for_each(|c| out.extend_from_slice(c));
        } else {
            out.extend_from_slice(ix);
        }
        pen = last(&out[at..]).unwrap_or(pen);
    }
    reorder(page, &out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use penkaku_core::font::{Font, Glyph};
    use penkaku_core::layout::{PageBox, PageParams, layout};
    use std::collections::HashMap;

    fn font() -> Font {
        let g = Glyph { advance: 4.0, strokes: vec![vec![(0.0, 0.0), (1.0, 0.0)], vec![(0.0, 1.0)]] };
        Font { glyphs: HashMap::from([('a', g.clone()), ('b', g.clone()), ('c', g)]) }
    }

    fn doc(text: &str) -> Document {
        let p = PageParams { page: Some(PageBox { width: 100.0, height: 200.0 }), ..Default::default() };
        layout(text, &font(), &p)
    }

    fn opts() -> OutputOptions {
        OutputOptions { path_plan: PathPlan::Raster, origin: Origin::Frame, rotation: PageRotation::None, sheet: Sheet { paper: (210.0, 297.0), margins: (20.0, 15.0, 10.0, 25.0) } }
    }

    /// Char index of each stroke, collapsed to one entry per character.
    fn char_order(p: &Page) -> Vec<usize> {
        let mut v = p.stroke_chars.clone();
        v.dedup();
        v
    }

    #[test]
    fn defaults_change_nothing() {
        let d = doc("abc\nabc");
        assert_eq!(plan(&d, 0, &opts()), d.pages[0]);
    }

    #[test]
    fn serpentine_reverses_the_second_row_only() {
        let d = doc("abc\nabc\nabc");
        let p = plan(&d, 0, &OutputOptions { path_plan: PathPlan::Serpentine, ..opts() });
        assert_eq!(char_order(&p), vec![0, 1, 2, 6, 5, 4, 8, 9, 10]);
        // Each character's strokes keep their order and shape.
        let src = &d.pages[0];
        let at = |pg: &Page, c: usize| pg.stroke_chars.iter().zip(&pg.strokes).filter(|(x, _)| **x == c).map(|(_, s)| s.clone()).collect::<Vec<_>>();
        for c in [0, 4, 6, 10] {
            assert_eq!(at(&p, c), at(src, c));
        }
        assert_eq!(p.strokes.len(), src.strokes.len());
        // A blank line doesn't count as a row.
        let d = doc("abc\n\nabc");
        let p = plan(&d, 0, &OutputOptions { path_plan: PathPlan::Serpentine, ..opts() });
        assert_eq!(char_order(&p), vec![0, 1, 2, 7, 6, 5]);
    }

    #[test]
    fn nearest_starts_each_row_at_the_nearer_end() {
        let near = OutputOptions { path_plan: PathPlan::Nearest, ..opts() };
        // Full rows: the pen ends each row at the far side, so this is 弓字型.
        let d = doc("abc\nabc\nabc");
        assert_eq!(char_order(&plan(&d, 0, &near)), vec![0, 1, 2, 6, 5, 4, 8, 9, 10]);
        // A one-char first row leaves the pen on the left: the second row runs left to right
        // (弓字型 would reverse it), the third comes back.
        let d = doc("a\nabc\nabc");
        assert_eq!(char_order(&plan(&d, 0, &near)), vec![0, 2, 3, 4, 8, 7, 6]);
        assert_eq!(char_order(&plan(&d, 0, &OutputOptions { path_plan: PathPlan::Serpentine, ..opts() })), vec![0, 4, 3, 2, 6, 7, 8]);
        // A turned page starts from the other corner: the first row runs right to left.
        let turned = OutputOptions { rotation: PageRotation::All180, ..near };
        assert_eq!(char_order(&plan(&doc("abc"), 0, &turned)), vec![2, 1, 0]);
    }

    #[test]
    fn serpentine_leaves_a_grid_alone() {
        let p = PageParams {
            page: Some(PageBox { width: 100.0, height: 200.0 }),
            ruling: penkaku_core::layout::Ruling::Grid { rows: 4, cols: 5, cell: (10.0, 10.0), gap: (0.0, 0.0) },
            ..Default::default()
        };
        let d = layout("abc\nabc", &font(), &p);
        assert!(d.grid);
        assert_eq!(plan(&d, 0, &OutputOptions { path_plan: PathPlan::Serpentine, ..opts() }), d.pages[0]);
    }

    #[test]
    fn turning_twice_is_identity_and_keeps_the_frame() {
        let d = doc("abc\nab");
        let o = OutputOptions { rotation: PageRotation::All180, ..opts() };
        let once = plan(&d, 0, &o);
        let mut d2 = d.clone();
        d2.pages[0] = once.clone();
        let twice = plan(&d2, 0, &o);
        for (a, b) in twice.strokes.iter().flatten().zip(d.pages[0].strokes.iter().flatten()) {
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4, "{a:?} vs {b:?}");
        }
        // Extents: same size, mirrored about the frame centre.
        let (w, h) = o.sheet.frame();
        let ext = |p: &Page| p.strokes.iter().flatten().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, e), &(x, y)| (a.min(x), b.min(y), c.max(x), e.max(y)));
        let (x0, y0, x1, y1) = ext(&d.pages[0]);
        let (u0, v0, u1, v1) = ext(&once);
        assert!(((u1 - u0) - (x1 - x0)).abs() < 1e-4 && ((v1 - v0) - (y1 - y0)).abs() < 1e-4);
        assert!((u0 - (w - x1)).abs() < 1e-4 && (v1 - (-h - y0)).abs() < 1e-4);
    }

    #[test]
    fn odd_and_even_rotation_pick_pages() {
        assert!(PageRotation::Odd180.turns(0) && !PageRotation::Odd180.turns(1));
        assert!(!PageRotation::Even180.turns(0) && PageRotation::Even180.turns(1));
    }

    #[test]
    fn two_colours_make_two_runs_in_first_seen_order() {
        let d = doc("abc\nabc");
        const RED: Rgb = [255, 0, 0];
        // Chars 1 and 5 red, the rest black.
        let colours = [None, Some(RED), None, None, None, Some(RED), None];
        let w = write(&d, 0, &colours, true, &opts());
        assert_eq!(w.pens, vec![(0, BLACK), (8, RED)]);
        let mut order = w.page.stroke_chars.clone();
        order.dedup();
        assert_eq!(order, vec![0, 2, 4, 6, 1, 5], "reading order inside each colour");
        assert_eq!(write(&d, 0, &colours, false, &opts()), Written { page: d.pages[0].clone(), ..Default::default() });
        // A one-colour page still names its pen, so the device can ask for it.
        assert_eq!(write(&d, 0, &[], true, &opts()).pens, vec![(0, BLACK)]);
    }

    #[test]
    fn marker_snaps_to_corners_or_whole_mm() {
        let s = opts().sheet;
        assert_eq!(s.snap_origin((1.0, -1.0), 2.0), Origin::Frame);
        assert_eq!(s.snap_origin((-19.0, 14.5), 2.0), Origin::Paper);
        assert_eq!(s.snap_origin((10.4, -24.6), 2.0), Origin::Custom { x: 30.0, y: 40.0 });
        assert_eq!(s.snap_origin((-50.0, 30.0), 2.0), Origin::Custom { x: 0.0, y: 0.0 }, "clamped to the paper");
    }

    #[test]
    fn origin_modes_shift_by_the_margins() {
        let d = doc("ab");
        let src = d.pages[0].strokes[0][0];
        let paper = plan(&d, 0, &OutputOptions { origin: Origin::Paper, ..opts() });
        assert_eq!(paper.strokes[0][0], (src.0 + 20.0, src.1 - 15.0));
        let custom = plan(&d, 0, &OutputOptions { origin: Origin::Custom { x: 30.0, y: 40.0 }, ..opts() });
        assert_eq!(custom.strokes[0][0], (src.0 - 10.0, src.1 + 25.0));
    }
}
