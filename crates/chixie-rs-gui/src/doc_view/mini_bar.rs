//! Word's floating mini toolbar: hovering the selected text shows 字体 / 字号 / A+ A− / 下划线 /
//! 倾斜 / 清除格式 just above the selection; it stays while the pointer is on it or its menus.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{DocView, ToggleUnderline, TiltRight, ClearFormat, ToggleTitle};
use crate::state::AppState;

/// Room the bar needs above the line, px.
const BAR_H: f32 = 40.0;

/// 字号 choices (ChiXie3 units, 100 = 20 pt).
const SIZES: [f32; 10] = [50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 140.0, 160.0];

impl DocView {
    /// Track whether the pointer is on the selected text (mouse moves without a drag).
    pub(super) fn track_hover(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let sel = self.doc.selection();
        let on = !sel.is_empty() && self.char_at_point(p).is_some_and(|i| sel.contains(&i));
        if on != self.mini_open {
            self.mini_open = on;
            cx.notify();
        }
    }

    pub fn mini_open(&self) -> bool {
        self.mini_open
    }
    /// Show (or hide) the bar without hovering (autotest).
    pub fn set_mini_open(&mut self, on: bool, cx: &mut Context<Self>) {
        self.mini_open = on;
        cx.notify();
    }

    /// Where the bar goes: above the selection's first line, or below it when there is no room
    /// above inside the page view (`true` = below).
    fn mini_bar_anchor(&self) -> Option<(Point<Pixels>, bool)> {
        let g = self.geom.get()?;
        let sel = self.doc.selection();
        let b = self.layout.boxes.get(sel.start)?;
        let l = self.layout.lines.get(b.line)?;
        let top = g.screen(b.page, b.x0, l.top);
        if top.y - px(BAR_H) < self.scroll.bounds().top() {
            Some((g.screen(b.page, b.x0, l.bottom), true))
        } else {
            Some((top, false))
        }
    }

    /// The bar, once the selection is hovered. It stays (the bar and its menus occlude the page)
    /// until the pointer moves over the page off the selection.
    pub(super) fn mini_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.doc.selection().is_empty() || self.selecting || !self.mini_open {
            return None;
        }
        let (at, below) = self.mini_bar_anchor()?;
        let theme = cx.theme();
        let (bg, border) = (theme.popover, theme.border);
        let fs = self.format_state(cx);
        let entity = cx.entity();
        let font_label = match &fs.font {
            Some(Some(name)) => name.clone(),
            Some(None) => "文档字体".to_string(),
            None => "（混合）".to_string(),
        };
        let size_label = fs.size.map_or("字号".to_string(), crate::app::fmt_num);
        let e = entity.clone();
        let font_btn = Button::new("mini-font").xsmall().ghost().label(font_label).dropdown_caret(true).dropdown_menu(move |m, _w, cx| {
            let st = cx.global::<AppState>();
            let main = st.active_font.as_ref().map(|f| f.name.clone());
            let fonts = st.fonts.list();
            let e0 = e.clone();
            let m = m.item(PopupMenuItem::new("文档字体（默认）").on_click(move |_, _w, cx| e0.update(cx, |d, cx| d.set_run_font(None, cx))));
            fonts.into_iter().fold(m, |m, f| {
                let e = e.clone();
                let name = (main.as_deref() != Some(f.name.as_str())).then(|| f.name.clone());
                m.item(PopupMenuItem::new(f.name).on_click(move |_, _w, cx| e.update(cx, |d, cx| d.set_run_font(name.clone(), cx))))
            })
        });
        let e = entity.clone();
        let size_btn = Button::new("mini-size").xsmall().ghost().label(size_label).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            SIZES.iter().fold(m, |m, &s| {
                let e = e.clone();
                m.item(PopupMenuItem::new(crate::app::fmt_num(s)).on_click(move |_, _w, cx| e.update(cx, |d, cx| d.set_run_size(s, cx))))
            })
        });
        let bump = |id: &'static str, label: &'static str, tip: &'static str, delta: f32| {
            let e = entity.clone();
            Button::new(id).xsmall().ghost().label(label).tooltip(tip).on_click(move |_: &ClickEvent, _w, cx| e.update(cx, |d, cx| d.bump_run_size(delta, cx)))
        };
        let act = |id: &'static str, icon: IconName, tip: &'static str, on: bool, a: fn() -> Box<dyn Action>| {
            Button::new(id).xsmall().ghost().selected(on).icon(Icon::new(icon)).tooltip(tip).on_click(move |_: &ClickEvent, w, cx| w.dispatch_action(a(), cx))
        };
        let bar = h_flex()
            .id("mini-bar")
            .gap_0p5()
            .p_0p5()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .shadow_md()
            .child(font_btn)
            .child(size_btn)
            .child(bump("mini-bigger", "A+", "增大字号", 10.0))
            .child(bump("mini-smaller", "A−", "缩小字号", -10.0))
            .child(act("mini-title", IconName::Heading, "标题 (Ctrl+B)", fs.title, || Box::new(ToggleTitle)))
            .child(act("mini-tilt", IconName::Italic, "倾斜 (Ctrl+I)", fs.tilt_right, || Box::new(TiltRight)))
            .child(act("mini-underline", IconName::Underline, "下划线 (Ctrl+U)", fs.underline, || Box::new(ToggleUnderline)))
            .child(act("mini-clear", IconName::RemoveFormatting, "清除格式", false, || Box::new(ClearFormat)));
        let wrap = div().id("mini-bar-wrap").occlude().map(|el| if below { el.pt_1() } else { el.pb_1() }).child(bar);
        let corner = if below { Anchor::TopLeft } else { Anchor::BottomLeft };
        Some(deferred(anchored().position(at).anchor(corner).snap_to_window().child(wrap)).with_priority(1).into_any_element())
    }
}
