//! The WYSIWYG page editor: pages stacked on a grey desk (Word "print layout"), handwriting drawn
//! live, caret and selection on the paper, keyboard/mouse/clipboard/undo, and IME through
//! `EntityInputHandler` (all platform ranges are UTF-16; `EditDoc` converts).
//!
//! Geometry: layout coordinates have the writable area's top-left at (0,0) with Y negative going
//! down. On paper that point is (margin_left, margin_top) mm. On screen, page k's top-left is at
//! `origin + (x0, GAP + k * (page_h + GAP))` px and 1 mm = `zoom` px.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chixie_rs_core::config::{PageSettings, Settings};
use chixie_rs_doc::EditDoc;
use chixie_rs_core::handwriting::StyleFile;
use chixie_rs_template::Template;
use chixie_rs_core::font::Font;
use chixie_rs_core::font::fix;
use chixie_rs_core::font::fontset::RunFonts;
use chixie_rs_core::layout::{self, Document};
use chixie_rs_doc::model::PT_PER_SIZE;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::notification::Notification;

use crate::state::AppState;

/// Gap between pages and around them, px.
const GAP: f32 = 24.0;
/// px per mm that shows paper at roughly its physical size on a 96-dpi screen.
pub const ZOOM_100: f32 = 96.0 / 25.4;
/// Zoom limits (px per mm).
pub const ZOOM_MIN: f32 = 0.4;
pub const ZOOM_MAX: f32 = 6.0;
pub const KEY_CONTEXT: &str = "ChixieRsEditor";

mod format;
pub use format::*;

actions!(
    chixie_editor,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, SelectLeft, SelectRight, SelectUp, SelectDown,
        WordLeft, WordRight, SelectWordLeft, SelectWordRight, LineStart, LineEnd, SelectLineStart,
        SelectLineEnd, DocStart, DocEnd, Backspace, Delete, BackspaceWord, DeleteWord, Newline,
        SelectAll, Copy, Cut, Paste, Undo, Redo, Save, AlignLeft, AlignCenter, AlignRight,
        AlignJustify, ToggleIndent, HalfSpace, Combine, NextPunctIssue, PrevCell,
    ]
);

/// Key bindings scoped to the editor: Windows/Linux style, with macOS's own text-navigation and
/// ⌘ shortcuts on macOS. Save works app-wide. (gpui-pre 0.3.11 has no `secondary-` key prefix.)
pub fn bind_keys(cx: &mut App) {
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([KeyBinding::new(if cfg!(target_os = "macos") { "cmd-s" } else { "ctrl-s" }, Save, None)]);
    format::bind_keys(cx);
    // Tab toggles the paragraph indent; Shift+Space types a half space (Ctrl+Space is the IME
    // toggle on Windows).
    // On a table form Tab goes to the next cell and Shift+Tab back.
    cx.bind_keys([KeyBinding::new("tab", ToggleIndent, c), KeyBinding::new("shift-tab", PrevCell, c), KeyBinding::new("shift-space", HalfSpace, c)]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("left", MoveLeft, c),
            KeyBinding::new("right", MoveRight, c),
            KeyBinding::new("up", MoveUp, c),
            KeyBinding::new("down", MoveDown, c),
            KeyBinding::new("shift-left", SelectLeft, c),
            KeyBinding::new("shift-right", SelectRight, c),
            KeyBinding::new("shift-up", SelectUp, c),
            KeyBinding::new("shift-down", SelectDown, c),
            KeyBinding::new("alt-left", WordLeft, c),
            KeyBinding::new("alt-right", WordRight, c),
            KeyBinding::new("alt-shift-left", SelectWordLeft, c),
            KeyBinding::new("alt-shift-right", SelectWordRight, c),
            KeyBinding::new("cmd-left", LineStart, c),
            KeyBinding::new("cmd-right", LineEnd, c),
            KeyBinding::new("cmd-shift-left", SelectLineStart, c),
            KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
            KeyBinding::new("home", LineStart, c),
            KeyBinding::new("end", LineEnd, c),
            KeyBinding::new("cmd-up", DocStart, c),
            KeyBinding::new("cmd-down", DocEnd, c),
            KeyBinding::new("backspace", Backspace, c),
            KeyBinding::new("delete", Delete, c),
            KeyBinding::new("alt-backspace", BackspaceWord, c),
            KeyBinding::new("alt-delete", DeleteWord, c),
            KeyBinding::new("enter", Newline, c),
            KeyBinding::new("cmd-a", SelectAll, c),
            KeyBinding::new("cmd-c", Copy, c),
            KeyBinding::new("cmd-x", Cut, c),
            KeyBinding::new("cmd-v", Paste, c),
            KeyBinding::new("cmd-z", Undo, c),
            KeyBinding::new("cmd-shift-z", Redo, c),
        ]);
        return;
    }
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, c),
        KeyBinding::new("right", MoveRight, c),
        KeyBinding::new("up", MoveUp, c),
        KeyBinding::new("down", MoveDown, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("ctrl-home", DocStart, c),
        KeyBinding::new("ctrl-end", DocEnd, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("ctrl-backspace", BackspaceWord, c),
        KeyBinding::new("ctrl-delete", DeleteWord, c),
        KeyBinding::new("enter", Newline, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
    ]);
}

/// Screen geometry of the last paint, for mouse hit-testing and IME candidate placement.
#[derive(Clone, Copy)]
struct Geom {
    origin: Point<Pixels>,
    x0: f32,
    page_w: f32,
    page_h: f32,
    zoom: f32,
    margin_l: f32,
    margin_t: f32,
    /// Pages per row: 1 (单页) or 2 (双页).
    cols: usize,
}

impl Geom {
    fn new(bounds: Bounds<Pixels>, page: &PageSettings, zoom: f32, cols: usize) -> Self {
        let (page_w, page_h) = (page.paper.0 * zoom, page.paper.1 * zoom);
        let row_w = cols as f32 * page_w + (cols - 1) as f32 * GAP;
        let x0 = ((f32::from(bounds.size.width) - row_w) / 2.0).max(GAP);
        Self { origin: bounds.origin, x0, page_w, page_h, zoom, margin_l: page.margins.0, margin_t: page.margins.1, cols }
    }
    fn page_origin(&self, k: usize) -> Point<Pixels> {
        let (row, col) = (k / self.cols, k % self.cols);
        self.origin + point(px(self.x0 + col as f32 * (self.page_w + GAP)), px(GAP + row as f32 * (self.page_h + GAP)))
    }
    /// Layout mm (y up, origin at writable top-left) → screen px on page `k`.
    fn screen(&self, k: usize, x: f32, y: f32) -> Point<Pixels> {
        self.page_origin(k) + point(px((self.margin_l + x) * self.zoom), px((self.margin_t - y) * self.zoom))
    }
    /// Screen px → (page, layout x, layout y).
    fn unproject(&self, p: Point<Pixels>, pages: usize) -> (usize, f32, f32) {
        let rel_y = f32::from(p.y - self.origin.y) - GAP / 2.0;
        let rel_x = f32::from(p.x - self.origin.x) - self.x0 + GAP / 2.0;
        let row = (rel_y / (self.page_h + GAP)).floor().max(0.0) as usize;
        let col = ((rel_x / (self.page_w + GAP)).floor().max(0.0) as usize).min(self.cols - 1);
        let k = (row * self.cols + col).min(pages.saturating_sub(1));
        let o = self.page_origin(k);
        let x = f32::from(p.x - o.x) / self.zoom - self.margin_l;
        let y = self.margin_t - f32::from(p.y - o.y) / self.zoom;
        (k, x, y)
    }
}

pub struct DocView {
    doc: EditDoc,
    layout: Arc<Document>,
    has_font: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
    zoom: f32,
    geom: Rc<Cell<Option<Geom>>>,
    selecting: bool,
    caret_visible: bool,
    reveal_caret: bool,
    /// 书写原点 marker being dragged: its layout point (mm).
    origin_drag: Option<(f32, f32)>,
    /// 格式刷 armed: the formats to give the next mouse selection, and whether it stays armed.
    painter: Option<(chixie_rs_doc::model::Attrs, bool)>,
    /// What the current layout was computed from, to skip needless relayouts.
    /// What the layout was made from: font set generation, page and style settings, template.
    layout_key: Option<(u64, PageSettings, StyleFile, Option<Template>)>,
    dirty_draft: bool,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl DocView {
    pub fn new(doc: EditDoc, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let sub = cx.observe_global::<AppState>(|this, cx| this.relayout_if_changed(cx));
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(530)).await;
                if this.update(cx, |v, cx| {
                    v.caret_visible = !v.caret_visible;
                    cx.notify();
                })
                .is_err()
                {
                    break;
                }
            }
        });
        let mut this = Self {
            doc,
            layout: Arc::new(Document::default()),
            has_font: false,
            focus,
            scroll: ScrollHandle::new(),
            zoom: ZOOM_100,
            geom: Rc::new(Cell::new(None)),
            selecting: false,
            caret_visible: true,
            reveal_caret: false,
            origin_drag: None,
            painter: None,
            layout_key: None,
            dirty_draft: false,
            _tasks: vec![blink],
            _subs: vec![sub],
        };
        this.relayout_if_changed(cx);
        this
    }

    // ---- state accessors used by the ribbon / status bar / file ops ----

    pub fn doc(&self) -> &EditDoc {
        &self.doc
    }
    pub fn document(&self) -> Arc<Document> {
        self.layout.clone()
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn set_zoom(&mut self, z: f32, cx: &mut Context<Self>) {
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        cx.notify();
    }
    pub fn caret_page(&self) -> usize {
        self.layout.caret(self.doc.caret()).page
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty_draft)
    }
    /// Fit the page width into the viewport (needs one paint).
    pub fn fit_width(&mut self, cx: &mut Context<Self>) {
        let vw = f32::from(self.scroll.bounds().size.width);
        let st = cx.global::<AppState>();
        let cols = if st.settings.ui.double_page { 2.0 } else { 1.0 };
        let paper_w = st.settings.page.paper.0;
        if vw > 100.0 {
            self.set_zoom((vw - (cols + 1.0) * GAP) / (cols * paper_w), cx);
        }
    }

    pub fn replace_doc(&mut self, doc: EditDoc, cx: &mut Context<Self>) {
        self.doc = doc;
        self.edited(cx);
    }

    /// Typed / pasted text at the selection (also used by the autotest).
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.doc.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
        self.edited(cx);
    }

    // ---- layout ----

    fn relayout_if_changed(&mut self, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        let key = (st.font_set_gen, st.settings.page.clone(), st.settings.style.clone(), st.active_template());
        if self.layout_key.as_ref() != Some(&key) {
            self.layout_key = Some(key);
            self.relayout(cx);
        }
    }

    fn relayout(&mut self, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        let params = st.settings.page_params_with(st.active_template().map(|t| t.ruling()).unwrap_or_default());
        // Every font in use (roles, mix, fallbacks); empty until one is chosen.
        let fonts = st.font_set.clone();
        self.has_font = !fonts.is_empty();
        self.load_run_fonts(cx);
        self.layout = Arc::new(crate::perf::time(crate::perf::Kind::Relayout, || self.lay_out(&params, cx)));
        cx.notify();
    }

    /// Lay the document out with `params`: text, paragraphs, 合并内容, 字符微调, 字符格式 and the
    /// characters' own fonts.
    fn lay_out(&self, params: &layout::PageParams, cx: &App) -> Document {
        let st = cx.global::<AppState>();
        let (joins, tuning) = (self.doc.joins(), self.doc.tunings());
        let styles = self.doc.styles(st.settings.page.font_size * PT_PER_SIZE);
        let by_id = self.doc.char_fonts().into_iter().filter_map(|(id, n)| st.run_fonts.get(n).cloned().flatten().map(|f| (id, f))).collect();
        let fonts = RunFonts { base: &*st.font_set, by_id };
        let flow = layout::Flow { chars: self.doc.chars(), ids: self.doc.ids(), paras: self.doc.document().paras(), joins: &joins, tuning: &tuning, styles: &styles };
        layout::layout_flow(&flow, &fonts, params)
    }

    /// Load the fonts characters name (字体) that are not cached yet; the main font is the
    /// document's own. A name with no local font is remembered as missing.
    fn load_run_fonts(&self, cx: &mut App) {
        let st = cx.global::<AppState>();
        let main = st.active_font.as_ref().map(|a| a.name.as_str());
        let mut names: Vec<String> = self.doc.char_fonts().into_iter().map(|(_, n)| n).filter(|n| Some(*n) != main && !st.run_fonts.contains_key(*n)).map(str::to_owned).collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return;
        }
        let (store, fixes) = (st.fonts.clone(), st.settings.fonts.fix);
        let loaded: Vec<(String, Option<Arc<Font>>)> = names
            .into_iter()
            .map(|n| {
                let font = store.id_of(&n).and_then(|id| store.load(id).inspect_err(|e| log::warn!("loading font {n}: {e}")).ok());
                let font = font.map(|f| Arc::new(if fixes.any() { fix::apply(&f, &fixes) } else { f }));
                (n, font)
            })
            .collect();
        cx.global_mut::<AppState>().run_fonts.extend(loaded);
    }

    /// After any text change: relayout, keep the caret visible, mark the draft dirty.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.relayout(cx);
        self.after_move(cx);
        self.dirty_draft = true;
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.caret_visible = true;
        self.reveal_caret = true;
        cx.notify();
    }

    /// Scroll so the caret is inside the viewport (run during paint, when geometry is fresh).
    fn reveal(&self, g: &Geom) {
        let c = self.layout.caret(self.doc.caret());
        let top = f32::from(g.screen(c.page, c.x, c.top).y - g.origin.y);
        let bottom = f32::from(g.screen(c.page, c.x, c.bottom).y - g.origin.y);
        let off = self.scroll.offset();
        let view_h = f32::from(self.scroll.bounds().size.height);
        let shown = -f32::from(off.y);
        if top < shown + 8.0 {
            self.scroll.set_offset(point(off.x, px(-(top - 8.0).max(0.0))));
        } else if bottom > shown + view_h - 8.0 {
            self.scroll.set_offset(point(off.x, px(-(bottom - view_h + 8.0))));
        }
    }

    // ---- actions ----

    fn move_by(&mut self, to: usize, extend: bool, cx: &mut Context<Self>) {
        self.doc.move_to(to, extend);
        self.after_move(cx);
    }
    fn line_edge(&self, end: bool) -> usize {
        let l = self.layout.lines[self.layout.line_of(self.doc.caret())];
        if end { l.end } else { l.start }
    }
    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.left(false);
        self.after_move(cx);
    }
    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.right(false);
        self.after_move(cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.left(true);
        self.after_move(cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.right(true);
        self.after_move(cx);
    }
    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.layout.vertical_move(self.doc.caret(), -1);
        self.move_by(to, false, cx);
    }
    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.layout.vertical_move(self.doc.caret(), 1);
        self.move_by(to, false, cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.layout.vertical_move(self.doc.caret(), -1);
        self.move_by(to, true, cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.layout.vertical_move(self.doc.caret(), 1);
        self.move_by(to, true, cx);
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.doc.word_left(self.doc.caret());
        self.move_by(to, false, cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.doc.word_right(self.doc.caret());
        self.move_by(to, false, cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.doc.word_left(self.doc.caret());
        self.move_by(to, true, cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.doc.word_right(self.doc.caret());
        self.move_by(to, true, cx);
    }
    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.line_edge(false);
        self.move_by(to, false, cx);
    }
    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.line_edge(true);
        self.move_by(to, false, cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.line_edge(false);
        self.move_by(to, true, cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.line_edge(true);
        self.move_by(to, true, cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_by(0, false, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.doc.len();
        self.move_by(n, false, cx);
    }
    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.backspace(false) {
            self.edited(cx);
        }
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.delete(false) {
            self.edited(cx);
        }
    }
    fn backspace_word(&mut self, _: &BackspaceWord, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.backspace(true) {
            self.edited(cx);
        }
    }
    fn delete_word(&mut self, _: &DeleteWord, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.delete(true) {
            self.edited(cx);
        }
    }
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.type_text("\n", cx);
    }
    fn half_space(&mut self, _: &HalfSpace, _: &mut Window, cx: &mut Context<Self>) {
        self.type_text(&layout::HALF_SPACE.to_string(), cx);
    }
    /// 字符微调 on the selection (or, with `same`, on every equal character).
    /// 笔色 of the selection (`None` = default black). One undo step.
    pub fn set_colour(&mut self, c: Option<chixie_rs_core::profile::Rgb>, cx: &mut Context<Self>) {
        self.doc.set_attrs(|a| a.colour = c);
        self.edited(cx);
    }

    pub fn tune(&mut self, f: fn(&mut layout::Tuning), same: bool, cx: &mut Context<Self>) {
        if same {
            self.doc.tune_same_chars(f);
        } else {
            self.doc.tune(f);
        }
        self.edited(cx);
    }

    /// 应用至同字符: every character equal to a selected one gets tuning `t`.
    pub fn set_tune_same(&mut self, t: layout::Tuning, cx: &mut Context<Self>) {
        self.doc.tune_same_chars(move |x| *x = t);
        self.edited(cx);
    }

    pub fn set_align(&mut self, align: layout::Align, cx: &mut Context<Self>) {
        self.doc.set_align(align);
        self.edited(cx);
    }
    fn align_left(&mut self, _: &AlignLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.set_align(layout::Align::Left, cx);
    }
    fn align_center(&mut self, _: &AlignCenter, _: &mut Window, cx: &mut Context<Self>) {
        self.set_align(layout::Align::Center, cx);
    }
    fn align_right(&mut self, _: &AlignRight, _: &mut Window, cx: &mut Context<Self>) {
        self.set_align(layout::Align::Right, cx);
    }
    fn align_justify(&mut self, _: &AlignJustify, _: &mut Window, cx: &mut Context<Self>) {
        self.set_align(layout::Align::Justify, cx);
    }
    /// Tab / 首行缩进: the width comes from the handwriting style's range, keyed by the
    /// paragraph's first char, so toggling it off and on again gives the same indent.
    pub fn toggle_indent(&mut self, cx: &mut Context<Self>) {
        let style = cx.global::<AppState>().settings.style.handwriting.clone();
        self.doc.toggle_indent(|id| style.indent_width(id));
        self.edited(cx);
    }
    fn toggle_indent_action(&mut self, _: &ToggleIndent, _: &mut Window, cx: &mut Context<Self>) {
        if table_mode(cx) {
            self.doc.next_cell();
            self.edited(cx);
        } else {
            self.toggle_indent(cx);
        }
    }
    fn prev_cell(&mut self, _: &PrevCell, _: &mut Window, cx: &mut Context<Self>) {
        if table_mode(cx) {
            self.doc.prev_cell();
            self.after_move(cx);
        }
    }

    // ---- table forms (see `table_ui`) ----

    /// Relayout now (the template changed under the document).
    pub fn relayout_now(&mut self, cx: &mut Context<Self>) {
        self.layout_key = None;
        self.relayout_if_changed(cx);
    }
    pub fn layout_doc(&self) -> &Document {
        &self.layout
    }
    pub fn doc_chars(&self) -> &[char] {
        self.doc.chars()
    }
    pub fn caret_index(&self) -> usize {
        self.doc.caret()
    }
    /// Several replacements as one undo step (刷新数据, 批量建页).
    pub fn replace_many(&mut self, edits: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) {
        self.doc.replace_many(edits);
        self.edited(cx);
    }
    /// Delete chars `r` (one undo step).
    pub fn delete_range(&mut self, r: Range<usize>, cx: &mut Context<Self>) {
        self.doc.select_range(r);
        self.doc.insert("");
        self.edited(cx);
    }
    /// Select chars `r` and scroll to them (查找).
    /// Scroll the desk to `y` px from the top (autotest).
    pub fn scroll_to(&mut self, y: f32, cx: &mut Context<Self>) {
        let x = self.scroll.offset().x;
        self.scroll.set_offset(point(x, px(-y)));
        cx.notify();
    }

    pub fn select(&mut self, r: Range<usize>, cx: &mut Context<Self>) {
        self.doc.select_range(r);
        self.after_move(cx);
    }
    /// 导入: add chars by `mode` (one undo step).
    pub fn import_chars(&mut self, chars: Vec<(char, Option<chixie_rs_doc::model::Attrs>)>, mode: chixie_rs_doc::import::ImportMode, cx: &mut Context<Self>) {
        let chars = if table_mode(cx) { chars.into_iter().map(|(c, a)| (if c == '\t' { layout::CELL_BREAK } else { c }, a)).collect() } else { chars };
        self.doc.import_chars(chars, mode);
        self.edited(cx);
    }
    /// Replace several ranges (替换 / 全部替换, one undo step).
    pub fn replace_ranges(&mut self, edits: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) {
        self.doc.replace_many(edits);
        self.edited(cx);
    }
    /// How segment `seg` would be set on template `t` (a trial layout; nothing changes).
    pub fn trial_cell(&self, t: &Template, seg: usize, cx: &App) -> Option<layout::CellInfo> {
        let st = cx.global::<AppState>();
        let params = st.settings.page_params_with(t.ruling());
        self.lay_out(&params, cx).cells.into_iter().find(|c| c.seg == seg)
    }
    fn combine(&mut self, _: &Combine, window: &mut Window, cx: &mut Context<Self>) {
        match self.doc.toggle_combine() {
            Ok(()) => self.edited(cx),
            Err(msg) => window.push_notification(Notification::warning(msg), cx),
        }
    }
    /// 位置合理性检测: put the caret at the start of the next line with a misplaced mark.
    fn next_punct_issue(&mut self, _: &NextPunctIssue, _: &mut Window, cx: &mut Context<Self>) {
        let caret_line = self.layout.line_of(self.doc.caret());
        let issues = &self.layout.punct_issues;
        if let Some(&l) = issues.iter().find(|&&l| l > caret_line).or(issues.first()) {
            self.doc.move_to(self.layout.lines[l].start, false);
            self.after_move(cx);
        }
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.select_all();
        self.after_move(cx);
    }
    /// The selection as plain text plus each char's formatting (JSON metadata, read back by
    /// our own paste; other programs see the text).
    fn copy_selection(&self, cx: &mut Context<Self>) {
        let (text, attrs) = (self.doc.selected_text(), self.doc.selected_attrs());
        cx.set_global(LastRichClip(Some((text.clone(), attrs.clone()))));
        cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(text, RichClip { v: 1, attrs }));
    }
    pub fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.has_selection() {
            self.copy_selection(cx);
        }
    }
    pub fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.has_selection() {
            self.copy_selection(cx);
            self.doc.insert("");
            self.edited(cx);
        }
    }
    pub fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else { return };
        // Our own copy keeps its formatting. Clipboard tools (Windows clipboard history,
        // PowerToys) may re-put the text without our metadata: then the same text as our last
        // copy still gets that copy's formatting.
        let rich = item
            .entries()
            .iter()
            .find_map(|e| match e {
                ClipboardEntry::String(s) => s.metadata_json::<RichClip>().filter(|c| c.v == 1).map(|c| (s.text().clone(), c.attrs)),
                _ => None,
            })
            .or_else(|| {
                let text = item.text()?;
                cx.try_global::<LastRichClip>().and_then(|l| l.0.clone()).filter(|(t, _)| *t == text)
            });
        if let Some((text, attrs)) = rich {
            self.doc.insert_with_attrs(&text, &attrs);
            self.edited(cx);
            return;
        }
        if let Some(text) = item.text() {
            // Tab-separated text (from a spreadsheet) fills cells on a table form.
            let text = if table_mode(cx) { text.replace('\t', &layout::CELL_BREAK.to_string()) } else { text };
            self.type_text(&text, cx);
        }
    }
    pub fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.undo() {
            self.edited(cx);
        }
    }
    pub fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.edited(cx);
        }
    }

    // ---- mouse ----

    fn index_at_point(&self, p: Point<Pixels>) -> Option<usize> {
        let g = self.geom.get()?;
        let (k, x, y) = g.unproject(p, self.layout.pages.len());
        Some(self.layout.index_at(k, x, y))
    }

    /// Layout point of the 书写原点 marker.
    fn origin_point(&self, cx: &App) -> (f32, f32) {
        self.origin_drag.unwrap_or_else(|| sheet(&cx.global::<AppState>().settings.page).origin_point(cx.global::<AppState>().settings.page.origin))
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        // An overflowing cell's warning badge opens the 内容溢出 dialog.
        if let Some(g) = self.geom.get() {
            let (k, x, y) = g.unproject(e.position, self.layout.pages.len().max(1));
            let hit = self.layout.overflowing().find(|c| c.page == k && x >= c.rect.0 && x <= c.rect.0 + BADGE_MM && y <= c.rect.1 && y >= c.rect.1 - BADGE_MM).copied();
            if let Some(info) = hit {
                crate::table_ui::open_overflow(cx.entity(), info, window, cx);
                return;
            }
        }
        // Grab the 书写原点 marker (drawn on every page; drag it on any).
        if let Some(g) = self.geom.get() {
            let (_, x, y) = g.unproject(e.position, self.layout.pages.len().max(1));
            let o = self.origin_point(cx);
            if (x - o.0).hypot(y - o.1) <= ORIGIN_GRAB_MM.max(8.0 / g.zoom) {
                self.origin_drag = Some(o);
                cx.notify();
                return;
            }
        }
        let Some(i) = self.index_at_point(e.position) else { return };
        if e.click_count >= 2 {
            let r = self.doc.word_at(i);
            self.doc.select_range(r);
        } else {
            self.doc.move_to(i, e.modifiers.shift);
            self.selecting = true;
        }
        self.after_move(cx);
    }
    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.origin_drag.is_some()
            && let Some(g) = self.geom.get()
        {
            let (_, x, y) = g.unproject(e.position, self.layout.pages.len().max(1));
            self.origin_drag = Some((x, y));
            cx.notify();
            return;
        }
        if self.selecting
            && let Some(i) = self.index_at_point(e.position)
        {
            self.doc.move_to(i, true);
            self.after_move(cx);
        }
    }
    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.selecting) {
            self.apply_painter(cx);
        }
        if let Some(p) = self.origin_drag.take() {
            let st = cx.global_mut::<AppState>();
            st.settings.page.origin = sheet(&st.settings.page).snap_origin(p, ORIGIN_SNAP_MM);
            st.save_settings();
            cx.notify();
        }
    }
    fn scroll_zoom(&mut self, e: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.modifiers.secondary(cx) {
            let dy = f32::from(e.delta.pixel_delta(px(16.)).y);
            self.set_zoom(self.zoom * if dy > 0.0 { 1.1 } else { 1.0 / 1.1 }, cx);
            cx.stop_propagation();
        }
    }

    fn caret_px_rect(&self, g: &Geom, i: usize) -> Bounds<Pixels> {
        let c = self.layout.caret(i);
        Bounds::from_corners(g.screen(c.page, c.x, c.top), g.screen(c.page, c.x, c.bottom) + point(px(2.0), px(0.0)))
    }
}

impl Focusable for DocView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for DocView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.doc.range_from_utf16(&range);
        *adjusted = Some(self.doc.range_to_utf16(&r));
        Some(self.doc.chars()[r].iter().collect())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.doc.range_to_utf16(&self.doc.selection()), reversed: self.doc.selection_reversed() })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.doc.marked().map(|r| self.doc.range_to_utf16(&r))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.unmark();
        cx.notify();
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let r = range.map(|r| self.doc.range_from_utf16(&r));
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.doc.commit_text(r, &text);
        self.edited(cx);
        window.notify_text_input_changed(&self.focus);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = range.map(|r| self.doc.range_from_utf16(&r));
        // Selection inside the new text: UTF-16 → char offsets within `new_text`.
        let to_chars = |u: usize| {
            let mut acc = 0;
            for (i, c) in new_text.chars().enumerate() {
                if acc >= u {
                    return i;
                }
                acc += c.len_utf16();
            }
            new_text.chars().count()
        };
        let sel = new_selected.map(|s| to_chars(s.start)..to_chars(s.end));
        self.doc.set_marked(r, new_text, sel);
        self.edited(cx);
        window.notify_text_input_changed(&self.focus);
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let g = self.geom.get()?;
        Some(self.caret_px_rect(&g, self.doc.from_utf16(range.start)))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        self.index_at_point(point).map(|i| self.doc.to_utf16(i))
    }
}

impl Render for DocView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::ActiveTheme as _;
        let theme = cx.theme();
        let desk = crate::ribbon::chrome(cx).desk;
        let page_shadow = [
            BoxShadow { color: hsla(0., 0., 0., 0.10), offset: point(px(0.), px(1.)), blur_radius: px(2.), spread_radius: px(0.), inset: false },
            BoxShadow { color: hsla(0., 0., 0., 0.12), offset: point(px(0.), px(3.)), blur_radius: px(10.), spread_radius: px(0.), inset: false },
        ];
        let (sel_color, caret_color, guide, missing_color) = (theme.selection, theme.primary, theme.border, theme.danger);
        let settings: &Settings = &cx.global::<AppState>().settings;
        let page = settings.page.clone();
        let page_prepaint = page.clone();
        let pages = self.layout.pages.len().max(1);
        let ui = settings.ui.clone();
        let cols = if ui.double_page { 2 } else { 1 };
        let rows = pages.div_ceil(cols);
        let total_h = GAP + rows as f32 * (page.paper.1 * self.zoom + GAP);
        let total_w = cols as f32 * page.paper.0 * self.zoom + (cols + 1) as f32 * GAP;

        let entity = cx.entity();
        let focus = self.focus.clone();
        let focused = focus.is_focused(window);
        let layout_doc = self.layout.clone();
        let ink_of: Vec<Option<chixie_rs_core::profile::Rgb>> = {
            let d = self.doc.document();
            (0..self.doc.len()).map(|i| d.attrs(i).colour).collect()
        };
        let sel = self.doc.selection();
        let caret = self.doc.caret();
        let marked = self.doc.marked();
        let show_caret = focused && self.caret_visible && sel.is_empty();
        let zoom = self.zoom;
        let show_guides = cx.global::<AppState>().settings.ui.show_guides;
        let origin_pt = self.origin_point(cx);
        let geom_out = self.geom.clone();
        let menu_geom = self.geom.clone();
        let reveal = std::mem::take(&mut self.reveal_caret);
        let scroll = self.scroll.clone();
        let this_reveal = entity.clone();

        div()
            .id("doc-desk")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(desk)
            .overflow_scroll()
            .track_scroll(&scroll)
            .map(|el| Self::format_actions(el, cx))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::backspace_word))
            .on_action(cx.listener(Self::delete_word))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::half_space))
            .on_action(cx.listener(Self::align_left))
            .on_action(cx.listener(Self::align_center))
            .on_action(cx.listener(Self::align_right))
            .on_action(cx.listener(Self::align_justify))
            .on_action(cx.listener(Self::toggle_indent_action))
            .on_action(cx.listener(Self::prev_cell))
            .on_action(cx.listener(Self::combine))
            .on_action(cx.listener(Self::next_punct_issue))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            // A file dropped on the page is imported (导入).
            .on_drop(cx.listener(|_, paths: &ExternalPaths, w, cx| {
                if let Some(p) = paths.paths().first() {
                    crate::import_ui::import_path(cx.entity(), p.clone(), w, cx);
                }
            }))
            .on_scroll(cx.listener(Self::scroll_zoom))
            .cursor(CursorStyle::IBeam)
            .child(
                canvas(
                    move |bounds, _w, _cx| {
                        let g = Geom::new(bounds, &page_prepaint, zoom, cols);
                        geom_out.set(Some(g));
                        g
                    },
                    move |bounds, g, window, cx| {
                        let _timer = crate::perf::PaintTimer::start();
                        // Pages.
                        for k in 0..pages {
                            let o = g.page_origin(k);
                            let paper = Bounds::new(o, size(px(g.page_w), px(g.page_h)));
                            // Word's sheet: soft drop shadow on the grey desk.
                            window.paint_drop_shadows(paper, Corners::default(), &page_shadow);
                            window.paint_quad(fill(paper, gpui_kit::white()));
                            let ruled = layout_doc.pages.get(k).is_some_and(|p| !p.guides.is_empty());
                            if !ruled {
                                let (l, t, r, b) = page.margins;
                                let guides = Bounds::from_corners(
                                    o + point(px(l * g.zoom), px(t * g.zoom)),
                                    o + point(px(g.page_w - r * g.zoom), px(g.page_h - b * g.zoom)),
                                );
                                window.paint_quad(outline(guides, guide.opacity(0.5), BorderStyle::Dashed));
                            }
                        }
                        // 行号 / 页码 (preview only).
                        paint_numbers(window, cx, &g, &layout_doc, pages, ui.line_numbers, ui.page_numbers);
                        // Template ruling (pre-printed on the paper; never plotted).
                        if show_guides {
                            for (k, p) in layout_doc.pages.iter().enumerate() {
                                paint_guides(window, &p.guides, |x, y| g.screen(k, x, y), g.zoom);
                            }
                        }
                        // Selection.
                        for i in sel.clone() {
                            let bx = layout_doc.boxes[i];
                            let line = layout_doc.lines[bx.line];
                            let x1 = if bx.x1 > bx.x0 { bx.x1 } else { bx.x0 + 2.0 };
                            let rect = Bounds::from_corners(g.screen(bx.page, bx.x0, line.top), g.screen(bx.page, x1, line.bottom));
                            window.paint_quad(fill(rect, sel_color.opacity(0.35)));
                        }
                        // Missing glyphs: dashed placeholder boxes.
                        for bx in layout_doc.missing_at.iter().filter_map(|&i| layout_doc.boxes.get(i)) {
                            {
                                let line = layout_doc.lines[bx.line];
                                let pad = (line.top - line.bottom) * 0.2;
                                let rect = Bounds::from_corners(
                                    g.screen(bx.page, bx.x0 + 0.3, line.top - pad),
                                    g.screen(bx.page, bx.x1 - 0.3, line.bottom + pad),
                                );
                                window.paint_quad(outline(rect, missing_color, BorderStyle::Dashed));
                            }
                        }
                        // Ink.
                        let stroke_w = px((0.35 * g.zoom).clamp(0.6, 2.5));
                        for (k, p) in layout_doc.pages.iter().enumerate() {
                            if p.strokes.is_empty() {
                                continue;
                            }
                            let colour_at = |i: usize| p.stroke_chars.get(i).and_then(|&c| ink_of.get(c).copied().flatten());
                            let mut inks: Vec<Option<chixie_rs_core::profile::Rgb>> = vec![None];
                            for i in 0..p.strokes.len() {
                                if let Some(c) = colour_at(i)
                                    && !inks.contains(&Some(c))
                                {
                                    inks.push(Some(c));
                                }
                            }
                            for ink in inks {
                                let mut path = PathBuilder::stroke(stroke_w);
                                for s in p.strokes.iter().enumerate().filter(|(i, _)| colour_at(*i) == ink).map(|(_, s)| s) {
                                    let mut pts = s.iter().map(|&(x, y)| g.screen(k, x, y));
                                    let Some(first) = pts.next() else { continue };
                                    path.move_to(first);
                                    let mut drew = false;
                                    for q in pts {
                                        path.line_to(q);
                                        drew = true;
                                    }
                                    if !drew {
                                        path.line_to(first + point(px(0.6), px(0.6)));
                                    }
                                }
                                if let Ok(path) = path.build() {
                                    let colour = ink.map_or(gpui_kit::black(), |[r, g, b]| Hsla::from(Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }));
                                    window.paint_path(path, colour);
                                }
                            }
                        }
                        // 内容溢出: a warning badge in each overflowing cell's top-left corner.
                        for c in layout_doc.overflowing() {
                            let a = g.screen(c.page, c.rect.0, c.rect.1);
                            let side = px(BADGE_MM * g.zoom);
                            window.paint_quad(fill(Bounds::new(a, size(side, side)), missing_color));
                            window.paint_quad(fill(Bounds::new(a + point(side * 0.42, side * 0.18), size(side * 0.16, side * 0.45)), gpui_kit::white()));
                            window.paint_quad(fill(Bounds::new(a + point(side * 0.42, side * 0.7), size(side * 0.16, side * 0.14)), gpui_kit::white()));
                        }
                        // 书写原点: a blue ring where the pen tip must stand at job start.
                        let ring = px((1.6 * g.zoom).max(5.0));
                        for k in 0..pages {
                            let c = g.screen(k, origin_pt.0, origin_pt.1);
                            let b = Bounds::new(c - point(ring, ring), size(ring * 2.0, ring * 2.0));
                            window.paint_quad(quad(b, ring, transparent_black(), px(2.0), ORIGIN_BLUE, BorderStyle::Solid));
                        }
                        // IME composition underline.
                        if let Some(m) = marked.clone() {
                            for i in m {
                                let bx = layout_doc.boxes[i];
                                let line = layout_doc.lines[bx.line];
                                let y = line.bottom + (line.top - line.bottom) * 0.08;
                                let rect = Bounds::from_corners(g.screen(bx.page, bx.x0, y), g.screen(bx.page, bx.x1, y) + point(px(0.), px(1.5)));
                                window.paint_quad(fill(rect, caret_color));
                            }
                        }
                        // Caret.
                        if show_caret {
                            let c = layout_doc.caret(caret);
                            let rect = Bounds::from_corners(g.screen(c.page, c.x, c.top), g.screen(c.page, c.x, c.bottom) + point(px(2.0), px(0.0)));
                            window.paint_quad(fill(rect, caret_color));
                        }
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                        if reveal {
                            this_reveal.read(cx).reveal(&g);
                        }
                    },
                )
                .w_full()
                .min_w(px(total_w))
                .h(px(total_h)),
            )
            .context_menu(move |m, window, cx| {
                // On the 书写原点 marker: ChiXie3's origin menu; elsewhere the edit commands.
                let near_origin = menu_geom.get().is_some_and(|g| {
                    let (_, x, y) = g.unproject(window.mouse_position(), pages);
                    (x - origin_pt.0).hypot(y - origin_pt.1) <= ORIGIN_GRAB_MM.max(8.0 / g.zoom)
                });
                if near_origin {
                    let set = |o: chixie_rs_core::profile::Origin| {
                        move |_: &ClickEvent, _w: &mut Window, cx: &mut App| {
                            let st = cx.global_mut::<AppState>();
                            st.settings.page.origin = o;
                            st.save_settings();
                            cx.refresh_windows();
                        }
                    };
                    use chixie_rs_core::profile::Origin;
                    use gpui_kit::component::menu::PopupMenuItem;
                    m.label("书写原点：请确保笔尖在此位置处")
                        .item(PopupMenuItem::new("移至纸张起点").on_click(set(Origin::Paper)))
                        .item(PopupMenuItem::new("移至框线起点").on_click(set(Origin::Frame)))
                        .item(PopupMenuItem::new("自定义原点位置：拖动蓝色圆圈").disabled(true))
                } else {
                    use crate::app::menu as am;
                    let table = table_mode(cx);
                    let m = m
                        .menu("剪切", Box::new(Cut))
                        .menu("复制", Box::new(Copy))
                        .menu("粘贴", Box::new(Paste))
                        .separator()
                        .menu("全选", Box::new(SelectAll))
                        .menu("查找…", Box::new(crate::find_bar::OpenFind))
                        .separator()
                        .submenu("书写此页", window, cx, |m, _w, _cx| m.menu("直接书写", Box::new(am::WritePage)).menu("上传SD卡书写", Box::new(am::WritePageSd)))
                        .menu("工作区巡边", Box::new(am::PatrolPage))
                        .menu("绘制框架线", Box::new(am::DrawFrame))
                        .menu("从这里断点续写", Box::new(am::ResumeHere));
                    if table {
                        m.separator()
                            .submenu("刷新数据", window, cx, |m, _w, _cx| m.menu("本页", Box::new(am::RefreshPage)).menu("所有页", Box::new(am::RefreshAll)))
                            .menu("批量溢出", Box::new(am::BatchOverflow))
                    } else {
                        m
                    }
                }
            })
    }
}

/// 行号 in the left margin (counted per page) and 第 n 页 under each page.
#[allow(clippy::too_many_arguments)]
fn paint_numbers(window: &mut Window, cx: &mut App, g: &Geom, d: &Document, pages: usize, lines: bool, page_nums: bool) {
    if !lines && !page_nums {
        return;
    }
    let size = px((3.0 * g.zoom).clamp(8.0, 12.0));
    let colour = hsla(0.0, 0.0, 0.55, 1.0);
    let font = window.text_style().font();
    let mut text = |window: &mut Window, s: String, at: Point<Pixels>, right: bool| {
        let s: SharedString = s.into();
        let run = TextRun { len: s.len(), font: font.clone(), color: colour, background_color: None, underline: None, strikethrough: None };
        let shaped = window.text_system().shape_line(s, size, &[run], None);
        let x = if right { at.x - shaped.width } else { at.x - shaped.width / 2.0 };
        let _ = shaped.paint(point(x, at.y - size * 0.6), size * 1.2, TextAlign::Left, None, window, cx);
    };
    if lines {
        let mut n = 0;
        let mut page = usize::MAX;
        for l in &d.lines {
            if l.page != page {
                (page, n) = (l.page, 0);
            }
            n += 1;
            let mid = g.screen(l.page, 0.0, (l.top + l.bottom) / 2.0);
            text(window, n.to_string(), mid - point(px(2.0 * g.zoom), px(0.0)), true);
        }
    }
    if page_nums {
        for k in 0..pages {
            let o = g.page_origin(k);
            // In the gap under the page, where the template's ruling can't cover it.
            let at = o + point(px(g.page_w / 2.0), px(g.page_h + GAP / 2.0));
            text(window, format!("第 {} 页", k + 1), at, false);
        }
    }
}

/// Clipboard metadata of a copy from the editor: each char's formatting.
#[derive(serde::Serialize, serde::Deserialize)]
struct RichClip {
    v: u32,
    attrs: Vec<chixie_rs_doc::model::Attrs>,
}

/// The editor's last copy (text and formatting), for when the clipboard comes back without
/// our metadata.
struct LastRichClip(Option<(String, Vec<chixie_rs_doc::model::Attrs>)>);

impl Global for LastRichClip {}

/// The 书写原点 marker: grab radius and corner snap (mm).
const ORIGIN_GRAB_MM: f32 = 2.5;
/// Side of the 内容溢出 badge (mm).
const BADGE_MM: f32 = 3.0;

/// The active template is a table form.
fn table_mode(cx: &App) -> bool {
    matches!(cx.global::<AppState>().active_template().map(|t| t.kind), Some(chixie_rs_template::TemplateKind::Table(_)))
}
const ORIGIN_SNAP_MM: f32 = 2.0;
const ORIGIN_BLUE: Hsla = Hsla { h: 0.58, s: 0.85, l: 0.5, a: 0.9 };

fn sheet(p: &PageSettings) -> chixie_rs_output::Sheet {
    chixie_rs_output::Sheet { paper: p.paper, margins: p.margins }
}

/// Light-blue ruling lines; dashed ones are cut into 1.2 mm dashes.
pub fn paint_guides(window: &mut Window, guides: &[chixie_rs_core::layout::Guide], to_px: impl Fn(f32, f32) -> Point<Pixels>, zoom: f32) {
    if guides.is_empty() {
        return;
    }
    let color = hsla(210.0 / 360.0, 0.65, 0.55, 0.55);
    let mut path = PathBuilder::stroke(px(1.0));
    for gd in guides {
        if gd.dashed {
            let (dx, dy) = (gd.b.0 - gd.a.0, gd.b.1 - gd.a.1);
            let len = (dx * dx + dy * dy).sqrt();
            let n = (len / 1.2).floor().max(1.0) as usize;
            for k in (0..n).step_by(2) {
                let t0 = k as f32 / n as f32;
                let t1 = ((k + 1) as f32 / n as f32).min(1.0);
                path.move_to(to_px(gd.a.0 + dx * t0, gd.a.1 + dy * t0));
                path.line_to(to_px(gd.a.0 + dx * t1, gd.a.1 + dy * t1));
            }
        } else {
            path.move_to(to_px(gd.a.0, gd.a.1));
            path.line_to(to_px(gd.b.0, gd.b.1));
        }
    }
    let _ = zoom;
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod geom_tests {
    use super::*;

    #[core::prelude::v1::test]
    fn double_page_places_pages_in_rows_of_two_and_hit_tests_back() {
        let page = PageSettings::default();
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(2000.), px(1000.)));
        let g = Geom::new(bounds, &page, 2.0, 2);
        let (o0, o1, o2) = (g.page_origin(0), g.page_origin(1), g.page_origin(2));
        assert_eq!(o0.y, o1.y);
        assert!(o1.x > o0.x && o2.x == o0.x && o2.y > o0.y);
        // A point in the writable area of each page maps back to that page.
        for k in 0..3 {
            let (pk, x, y) = g.unproject(g.screen(k, 10.0, -10.0), 3);
            assert_eq!(pk, k);
            assert!((x - 10.0).abs() < 1e-3 && (y + 10.0).abs() < 1e-3);
        }
        // Single page: one column.
        let g1 = Geom::new(bounds, &page, 2.0, 1);
        assert_eq!(g1.page_origin(1).x, g1.page_origin(0).x);
    }
}
