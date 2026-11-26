//! 查找 / 替换 bar above the page (Word style): Ctrl+F finds, Ctrl+H also replaces. Enter goes to
//! the next match, Shift+Enter to the previous one, Esc closes. Messages are ChiXie3's.

use std::ops::Range;

use penkaku_doc::FindOpts;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::doc_view::DocView;

actions!(chixie_find, [OpenFind, OpenReplace]);

pub fn bind_keys(cx: &mut App) {
    if cfg!(target_os = "macos") {
        cx.bind_keys([KeyBinding::new("cmd-f", OpenFind, None), KeyBinding::new("cmd-alt-f", OpenReplace, None)]);
    } else {
        cx.bind_keys([KeyBinding::new("ctrl-f", OpenFind, None), KeyBinding::new("ctrl-h", OpenReplace, None)]);
    }
}

pub enum FindBarEvent {
    Close,
}

impl EventEmitter<FindBarEvent> for FindBar {}

pub struct FindBar {
    doc: Entity<DocView>,
    find: Entity<InputState>,
    replace: Entity<InputState>,
    opts: FindOpts,
    pub replace_mode: bool,
    _subs: Vec<Subscription>,
}

impl FindBar {
    pub fn new(doc: Entity<DocView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("查找内容"));
        let replace = cx.new(|cx| InputState::new(window, cx).placeholder("替换为"));
        let subs = vec![
            cx.subscribe_in(&find, window, |this, _s, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { shift, .. } => this.step(!*shift, window, cx),
                _ => {}
            }),
            cx.subscribe_in(&replace, window, |this, _s, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.replace_one(window, cx);
                }
            }),
            cx.observe(&doc, |_, _, cx| cx.notify()),
        ];
        Self { doc, find, replace, opts: FindOpts::default(), replace_mode: false, _subs: subs }
    }

    /// Show the bar: the selection (one line) becomes the search text; focus goes to it.
    pub fn open(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_mode = replace;
        let d = self.doc.read(cx).doc();
        let sel = d.selection();
        let picked: String = d.chars()[sel].iter().collect();
        if !picked.is_empty() && !picked.contains('\n') && picked.chars().count() <= 64 {
            self.find.update(cx, |s, cx| s.set_value(picked, window, cx));
        }
        let focus = self.find.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        // Word: the search text comes selected, so typing replaces it.
        self.find.update(cx, |s, cx| s.select_all(window, cx));
        cx.notify();
    }

    fn needle(&self, cx: &App) -> String {
        self.find.read(cx).value().to_string()
    }

    fn hits(&self, cx: &App) -> Vec<Range<usize>> {
        self.doc.read(cx).doc().find_all(&self.needle(cx), self.opts)
    }

    /// Next (or previous) match from the selection, wrapping around.
    fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        if hits.is_empty() {
            window.push_notification(Notification::info("未搜索到结果"), cx);
            return;
        }
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

    /// 替换: replace the selected match, then go to the next one.
    fn replace_one(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        if hits.is_empty() {
            window.push_notification(Notification::info("未查询到结果，无法替换"), cx);
            return;
        }
        let sel = self.doc.read(cx).doc().selection();
        if hits.contains(&sel) {
            let with = self.replace.read(cx).value().to_string();
            let n = with.chars().count();
            self.doc.update(cx, |d, cx| {
                d.replace_ranges(vec![(sel.clone(), with)], cx);
                d.select(sel.start + n..sel.start + n, cx);
            });
        }
        self.step(true, window, cx);
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        if hits.is_empty() {
            window.push_notification(Notification::info("未查询到结果，无法替换"), cx);
            return;
        }
        let with = self.replace.read(cx).value().to_string();
        let n = hits.len();
        self.doc.update(cx, |d, cx| d.replace_ranges(hits.into_iter().map(|r| (r, with.clone())).collect(), cx));
        window.push_notification(Notification::success(format!("全部完成，共替换 {n} 处")), cx);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FindBarEvent::Close);
        self.doc.update(cx, |d, cx| d.focus(window, cx));
    }
}

impl Render for FindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, muted) = (theme.background, theme.border, theme.muted_foreground);
        let hits = self.hits(cx);
        let sel = self.doc.read(cx).doc().selection();
        let count = if self.needle(cx).is_empty() {
            String::new()
        } else if hits.is_empty() {
            "无结果".to_string()
        } else {
            match hits.iter().position(|h| *h == sel) {
                Some(k) => format!("第 {} / {} 处", k + 1, hits.len()),
                None => format!("共 {} 处", hits.len()),
            }
        };
        let o = self.opts;
        h_flex()
            .id("find-bar")
            .w_full()
            .px_2()
            .py_1()
            .gap_2()
            .bg(bg)
            .border_b_1()
            .border_color(border)
            .capture_key_down(cx.listener(|t, e: &KeyDownEvent, w, cx| {
                if e.keystroke.key == "escape" {
                    t.close(w, cx);
                    cx.stop_propagation();
                }
            }))
            .child(div().w(px(220.)).child(Input::new(&self.find).small()))
            .child(div().min_w(px(72.)).text_xs().text_color(muted).child(count))
            .child(Button::new("find-prev").ghost().xsmall().icon(IconName::ChevronUp).tooltip("上一处 (Shift+Enter)").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.step(false, w, cx))))
            .child(Button::new("find-next").ghost().xsmall().icon(IconName::ChevronDown).tooltip("下一处 (Enter)").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.step(true, w, cx))))
            .child(Checkbox::new("find-case").small().label("区分大小写").checked(o.match_case).on_click(cx.listener(|t, on: &bool, _w, cx| {
                t.opts.match_case = *on;
                cx.notify();
            })))
            .child(Checkbox::new("find-width").small().label("全角半角等同").checked(o.fold_width).on_click(cx.listener(|t, on: &bool, _w, cx| {
                t.opts.fold_width = *on;
                cx.notify();
            })))
            .when(self.replace_mode, |el| {
                el.child(div().w(px(180.)).child(Input::new(&self.replace).small()))
                    .child(Button::new("replace-one").small().label("替换").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.replace_one(w, cx))))
                    .child(Button::new("replace-all").small().label("全部替换").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.replace_all(w, cx))))
            })
            .when(!self.replace_mode, |el| {
                el.child(Button::new("show-replace").ghost().xsmall().label("替换…").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                    t.replace_mode = true;
                    cx.notify();
                })))
            })
            .child(div().flex_1())
            .child(Button::new("find-close").ghost().xsmall().icon(IconName::X).tooltip("关闭 (Esc)").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.close(w, cx))))
    }
}
