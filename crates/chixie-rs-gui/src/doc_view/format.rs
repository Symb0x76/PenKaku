//! 字符格式 on the editor: toggles (下划线 / 删除线 / 上标 / 下标 / 标题), 倾斜, the selection's
//! font and size, 清除格式 and 格式刷. Each change is one undo step on the selected chars.

use chixie_rs_doc::model::{Attrs, PT_PER_SIZE, Script};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use super::{DocView, KEY_CONTEXT};
use crate::state::AppState;

actions!(chixie_editor, [ToggleUnderline, ToggleStrike, ToggleSuper, ToggleSub, ToggleTitle, TiltRight, TiltLeft, ClearFormat, CancelPainter]);

/// Word's shortcuts (Ctrl+B is 标题 here: there is no bold pen).
pub fn bind_keys(cx: &mut App) {
    let c = Some(KEY_CONTEXT);
    let mac = cfg!(target_os = "macos");
    let k = |win: &'static str, mac_key: &'static str| if mac { mac_key } else { win };
    cx.bind_keys([
        KeyBinding::new(k("ctrl-u", "cmd-u"), ToggleUnderline, c),
        KeyBinding::new(k("ctrl-i", "cmd-i"), TiltRight, c),
        KeyBinding::new(k("ctrl-b", "cmd-b"), ToggleTitle, c),
        KeyBinding::new(k("ctrl-shift-=", "cmd-shift-="), ToggleSuper, c),
        KeyBinding::new(k("ctrl-+", "cmd-+"), ToggleSuper, c),
        KeyBinding::new(k("ctrl-=", "cmd-="), ToggleSub, c),
        KeyBinding::new("escape", CancelPainter, c),
    ]);
}

/// The selection's formats as the ribbon shows them. `font` / `size`: `None` when the selected
/// chars differ (shown blank); `Some(None)` font = the document's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormatState {
    pub selected: bool,
    pub font: Option<Option<String>>,
    /// ChiXie3 size units.
    pub size: Option<f32>,
    pub underline: bool,
    pub strike: bool,
    pub sup: bool,
    pub sub: bool,
    pub title: bool,
    pub tilt_right: bool,
    pub tilt_left: bool,
    pub painter: bool,
}

impl DocView {
    pub(super) fn format_actions(el: Stateful<Div>, cx: &Context<Self>) -> Stateful<Div> {
        el.on_action(cx.listener(|t, _: &ToggleUnderline, w, cx| t.toggle_format(|a| a.underline, |a, on| a.underline = on, w, cx)))
            .on_action(cx.listener(|t, _: &ToggleStrike, w, cx| t.toggle_format(|a| a.strike, |a, on| a.strike = on, w, cx)))
            .on_action(cx.listener(|t, _: &ToggleSuper, w, cx| {
                t.toggle_format(|a| a.script == Script::Super, |a, on| a.script = if on { Script::Super } else { Script::None }, w, cx)
            }))
            .on_action(cx.listener(|t, _: &ToggleSub, w, cx| {
                t.toggle_format(|a| a.script == Script::Sub, |a, on| a.script = if on { Script::Sub } else { Script::None }, w, cx)
            }))
            .on_action(cx.listener(|t, _: &ToggleTitle, w, cx| t.toggle_format(|a| a.title, |a, on| a.title = on, w, cx)))
            .on_action(cx.listener(|t, _: &TiltRight, w, cx| t.toggle_format(|a| a.tilt > 0, |a, on| a.tilt = i8::from(on), w, cx)))
            .on_action(cx.listener(|t, _: &TiltLeft, w, cx| t.toggle_format(|a| a.tilt < 0, |a, on| a.tilt = -i8::from(on), w, cx)))
            .on_action(cx.listener(|t, _: &ClearFormat, w, cx| {
                if t.need_selection(w, cx) {
                    t.doc.clear_format();
                    t.edited(cx);
                }
            }))
            .on_action(cx.listener(|t, _: &CancelPainter, _w, cx| {
                if t.painter.take().is_some() {
                    cx.notify();
                }
            }))
    }

    fn need_selection(&self, window: &mut Window, cx: &mut App) -> bool {
        let ok = !self.doc.selection().is_empty();
        if !ok {
            window.push_notification(Notification::warning("请先选中文字"), cx);
        }
        ok
    }

    fn toggle_format(&mut self, has: fn(&Attrs) -> bool, set: fn(&mut Attrs, bool), window: &mut Window, cx: &mut Context<Self>) {
        if self.need_selection(window, cx) {
            self.doc.toggle_attr(has, set);
            self.edited(cx);
        }
    }

    /// The selection's formats for the ribbon.
    pub fn format_state(&self, cx: &App) -> FormatState {
        let d = &self.doc;
        let on = |f: fn(&Attrs) -> bool| d.common(f) == Some(true);
        let base = cx.global::<AppState>().settings.page.font_size;
        FormatState {
            selected: !d.selection().is_empty(),
            font: d.common(|a| a.font.clone()),
            size: d.common(|a| a.size_pt).map(|s| s.map_or(base, |pt| pt / PT_PER_SIZE)),
            underline: on(|a| a.underline),
            strike: on(|a| a.strike),
            sup: on(|a| a.script == Script::Super),
            sub: on(|a| a.script == Script::Sub),
            title: on(|a| a.title),
            tilt_right: on(|a| a.tilt > 0),
            tilt_left: on(|a| a.tilt < 0),
            painter: self.painter.is_some(),
        }
    }

    /// 字体 of the selection (`None` = the document's).
    pub fn set_run_font(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.doc.common(|a| a.font.clone()).as_ref() == Some(&name) {
            return;
        }
        self.doc.set_attrs(|a| a.font = name.clone());
        self.edited(cx);
    }

    /// 字号 of the selection, ChiXie3 units (100 = 20 pt).
    pub fn set_run_size(&mut self, size: f32, cx: &mut Context<Self>) {
        if self.format_state(cx).size == Some(size) {
            return;
        }
        self.doc.set_attrs(|a| a.size_pt = Some(size * PT_PER_SIZE));
        self.edited(cx);
    }

    /// A+ / A− on the selection: each char's size moves by `delta` units (ChiXie3 steps by 10).
    pub fn bump_run_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        let base = cx.global::<AppState>().settings.page.font_size * PT_PER_SIZE;
        self.doc.set_attrs(|a| a.size_pt = Some((a.size_pt.unwrap_or(base) + delta * PT_PER_SIZE).clamp(1.0, 200.0)));
        self.edited(cx);
    }

    /// 字符微调 A+ / A− (ChiXie3: 增大字号5%): each selected char's size times `factor`.
    pub fn scale_run_size(&mut self, factor: f32, cx: &mut Context<Self>) {
        let base = cx.global::<AppState>().settings.page.font_size * PT_PER_SIZE;
        self.doc.set_attrs(|a| a.size_pt = Some((a.size_pt.unwrap_or(base) * factor).clamp(1.0, 200.0)));
        self.edited(cx);
    }

    /// The library key of the font that draws the first selected char: its own 字体, else the
    /// font the font set picks for it.
    pub fn selected_font_key(&self, cx: &App) -> Option<chixie_rs_core::font::library::FontKey> {
        let i = self.doc.selection().start;
        let c = *self.doc.chars().get(i)?;
        let st = cx.global::<AppState>();
        match &self.doc.document().attrs(i).font {
            Some(name) => st.fonts.id_of(name).map(chixie_rs_core::font::library::FontKey::from_local_id),
            None => st.font_set.key_of(c, self.doc.ids()[i]).cloned(),
        }
    }

    /// The strokes the first selected char is drawn with (page mm, as laid out).
    pub fn selected_strokes(&self) -> Vec<chixie_rs_core::layout::Stroke> {
        let i = self.doc.selection().start;
        let Some(b) = self.layout.boxes.get(i) else { return Vec::new() };
        let Some(p) = self.layout.pages.get(b.page) else { return Vec::new() };
        p.strokes.iter().zip(&p.stroke_chars).filter(|(_, c)| **c == i).map(|(s, _)| s.clone()).collect()
    }

    /// 格式刷: take the formats at the selection start (or the caret); the next selection made
    /// with the mouse receives them. `sticky` (double-click) keeps it armed until Esc.
    pub fn arm_painter(&mut self, sticky: bool, cx: &mut Context<Self>) {
        let sel = self.doc.selection();
        let d = self.doc.document();
        let src = if sel.is_empty() { d.attrs_at(sel.start) } else { d.attrs(sel.start).clone() };
        self.painter = Some((src, sticky));
        cx.notify();
    }

    /// After a mouse selection: paint it when the 格式刷 is armed.
    pub(super) fn apply_painter(&mut self, cx: &mut Context<Self>) {
        let Some((src, sticky)) = self.painter.clone() else { return };
        if self.doc.selection().is_empty() {
            return;
        }
        self.doc.paint_format(&src);
        if !sticky {
            self.painter = None;
        }
        self.edited(cx);
    }
}
