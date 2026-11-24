//! 导航 pane on the left of the page (Word's navigation pane): 搜索 lists every match of the
//! search text and 缺字 every character no font has, each with its context and page / line.
//! Clicking a row selects it on the page and scrolls there.

use std::ops::Range;

use chixie_rs_core::layout::Document;
use chixie_rs_doc::FindOpts;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::doc_view::DocView;

/// Rows shown at most (the count above the list is always exact).
const MAX_ROWS: usize = 500;
/// Characters of context on each side of a hit.
const CONTEXT: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavTab {
    Search,
    Missing,
}

pub enum NavPaneEvent {
    Close,
}

impl EventEmitter<NavPaneEvent> for NavPane {}

pub struct NavPane {
    doc: Entity<DocView>,
    query: Entity<InputState>,
    opts: FindOpts,
    pub tab: NavTab,
    _subs: Vec<Subscription>,
}

/// One result row: the range to select and where it is.
struct Row {
    range: Range<usize>,
    before: String,
    hit: String,
    after: String,
    place: String,
}

impl NavPane {
    pub fn new(doc: Entity<DocView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("在文档中搜索"));
        let subs = vec![
            cx.subscribe_in(&query, window, |this, _s, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { shift, .. } => this.step(!*shift, cx),
                _ => {}
            }),
            cx.observe(&doc, |_, _, cx| cx.notify()),
        ];
        Self { doc, query, opts: FindOpts::default(), tab: NavTab::Search, _subs: subs }
    }

    /// Show `tab`; 搜索 takes the selection (one line) as its text and focuses the box.
    pub fn open(&mut self, tab: NavTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == NavTab::Search {
            let d = self.doc.read(cx).doc();
            let picked: String = d.chars()[d.selection()].iter().collect();
            if !picked.is_empty() && !picked.contains('\n') && picked.chars().count() <= 64 {
                self.query.update(cx, |s, cx| s.set_value(picked, window, cx));
            }
            let focus = self.query.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            self.query.update(cx, |s, cx| s.select_all(window, cx));
        }
        cx.notify();
    }

    pub fn hit_count(&self, cx: &App) -> usize {
        self.hits(cx).len()
    }

    fn hits(&self, cx: &App) -> Vec<Range<usize>> {
        let needle = self.query.read(cx).value().to_string();
        self.doc.read(cx).doc().find_all(&needle, self.opts)
    }

    /// Enter / Shift+Enter in the box: next (previous) match from the selection, wrapping.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        let sel = self.doc.read(cx).doc().selection();
        let pick = if forward {
            hits.iter().find(|h| h.start >= sel.end && *h != &sel).or(hits.first())
        } else {
            hits.iter().rev().find(|h| h.end <= sel.start).or(hits.last())
        };
        if let Some(r) = pick.cloned() {
            self.doc.update(cx, |d, cx| d.select(r, cx));
        }
    }

    fn rows(ranges: impl Iterator<Item = Range<usize>>, chars: &[char], layout: &Document) -> Vec<Row> {
        // Line number within its page, per layout line.
        let mut line_in_page = Vec::with_capacity(layout.lines.len());
        let (mut page, mut n) = (usize::MAX, 0);
        for l in &layout.lines {
            if l.page != page {
                (page, n) = (l.page, 0);
            }
            n += 1;
            line_in_page.push(n);
        }
        let flat = |s: &[char]| s.iter().map(|&c| if c == '\n' || c == '\t' { ' ' } else { c }).collect::<String>();
        ranges
            .take(MAX_ROWS)
            .map(|r| {
                let lo = r.start.saturating_sub(CONTEXT);
                let hi = (r.end + CONTEXT).min(chars.len());
                let place = match layout.boxes.get(r.start) {
                    Some(b) => format!("第 {} 页 第 {} 行", b.page + 1, line_in_page.get(b.line).copied().unwrap_or(0)),
                    None => String::new(),
                };
                Row { before: flat(&chars[lo..r.start]), hit: flat(&chars[r.clone()]), after: flat(&chars[r.end..hi]), place, range: r }
            })
            .collect()
    }

    fn row_el(&self, ix: usize, row: Row, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, hit_bg, hover, sel_bg, danger) = (theme.muted_foreground, theme.warning.opacity(0.35), theme.secondary_hover, theme.selection, theme.danger);
        let missing = self.tab == NavTab::Missing;
        let r = row.range.clone();
        v_flex()
            .id(("nav-row", ix))
            .w_full()
            .px_2()
            .py_1()
            .gap_0p5()
            .rounded_md()
            .cursor_pointer()
            .when(current, |el| el.bg(sel_bg))
            .hover(move |s| s.bg(hover))
            .child(
                h_flex()
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(row.before)
                    .child(div().px_0p5().rounded_sm().font_weight(FontWeight::SEMIBOLD).when(missing, |el| el.text_color(danger)).when(!missing, |el| el.bg(hit_bg)).child(row.hit))
                    .child(row.after),
            )
            .child(div().text_xs().text_color(muted).child(row.place))
            .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                let r = r.clone();
                t.doc.update(cx, |d, cx| d.select(r, cx));
            }))
            .into_any_element()
    }
}

impl Render for NavPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let card = crate::ribbon::chrome(cx).card_bg;
        let view = self.doc.read(cx);
        let layout = view.document();
        let chars = view.doc_chars().to_vec();
        let sel = view.doc().selection();
        let (summary, rows) = match self.tab {
            NavTab::Search => {
                let hits = self.hits(cx);
                let summary = if self.query.read(cx).value().is_empty() {
                    "输入文字后列出所有匹配处".to_string()
                } else if hits.is_empty() {
                    "无结果".to_string()
                } else {
                    format!("共 {} 处", hits.len())
                };
                (summary, Self::rows(hits.into_iter(), &chars, &layout))
            }
            NavTab::Missing => {
                let summary = if layout.missing.is_empty() {
                    "没有缺字".to_string()
                } else {
                    format!("缺 {} 种字，共 {} 处：{}", layout.missing.len(), layout.missing_at.len(), layout.missing.iter().collect::<String>())
                };
                (summary, Self::rows(layout.missing_at.iter().map(|&i| i..i + 1), &chars, &layout))
            }
        };
        let shown = rows.len();
        let o = self.opts;
        let list: Vec<AnyElement> = rows.into_iter().enumerate().map(|(ix, row)| {
            let current = row.range == sel;
            self.row_el(ix, row, current, cx)
        }).collect();
        v_flex()
            .id("nav-pane")
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .bg(card)
            .border_r_1()
            .border_color(border)
            .capture_key_down(cx.listener(|_t, e: &KeyDownEvent, _w, cx| {
                if e.keystroke.key == "escape" {
                    cx.emit(NavPaneEvent::Close);
                    cx.stop_propagation();
                }
            }))
            .child(
                h_flex()
                    .h(px(40.))
                    .px_3()
                    .flex_shrink_0()
                    .child(div().flex_1().text_base().font_weight(FontWeight::SEMIBOLD).child("导航"))
                    .child(crate::ribbon::icon_btn("nav-close", gpui_kit::assets::IconName::Close, "关闭 (Esc)").on_click(cx.listener(|_t, _: &ClickEvent, _w, cx| cx.emit(NavPaneEvent::Close)))),
            )
            .child(
                div().px_3().pb_2().child(
                    TabBar::new("nav-tabs")
                        .small()
                        .segmented()
                        .selected_index(if self.tab == NavTab::Search { 0 } else { 1 })
                        .children(["搜索", "缺字"].map(|l| Tab::new().label(l)))
                        .on_click(cx.listener(|t, ix: &usize, _w, cx| {
                            t.tab = if *ix == 0 { NavTab::Search } else { NavTab::Missing };
                            cx.notify();
                        })),
                ),
            )
            .when(self.tab == NavTab::Search, |el| {
                el.child(
                    v_flex()
                        .px_3()
                        .gap_1()
                        .child(Input::new(&self.query).small().cleanable(true))
                        .child(
                            h_flex()
                                .gap_3()
                                .child(Checkbox::new("nav-case").small().label("区分大小写").checked(o.match_case).on_click(cx.listener(|t, on: &bool, _w, cx| {
                                    t.opts.match_case = *on;
                                    cx.notify();
                                })))
                                .child(Checkbox::new("nav-width").small().label("全角半角等同").checked(o.fold_width).on_click(cx.listener(|t, on: &bool, _w, cx| {
                                    t.opts.fold_width = *on;
                                    cx.notify();
                                }))),
                        ),
                )
            })
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child(summary))
                    .when(self.tab == NavTab::Missing && !layout.missing.is_empty(), |el| {
                        let text: String = layout.missing.iter().collect();
                        el.child(Button::new("nav-copy-missing").ghost().xsmall().label("复制").on_click(move |_: &ClickEvent, _w, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        }))
                    }),
            )
            .child(
                v_flex()
                    .id("nav-list")
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .overflow_y_scroll()
                    .children(list)
                    .when(shown == MAX_ROWS, |el| el.child(div().p_2().text_xs().text_color(muted).child(format!("只列出前 {MAX_ROWS} 处")))),
            )
    }
}
