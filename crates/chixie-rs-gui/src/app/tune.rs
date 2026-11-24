//! 字符微调: a task-pane card that nudges, resizes or re-deforms the selected character(s). Every
//! click is one undoable edit; 应用至同字符 repeats it on every equal character.

use chixie_rs_core::layout::Tuning;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::state::AppState;

/// One click of the arrow pad (ChiXie3: 上移0.2 …).
pub(crate) const STEP_MM: f32 = 0.2;
/// A+ / A− and 变胖 / 变瘦 (ChiXie3: 增大字号5%).
pub(crate) const STEP_PCT: f32 = 0.05;

type Change = fn(&mut Tuning);

impl ChixieRsApp {
    pub(crate) fn open_tune(&mut self, cx: &mut Context<Self>) {
        (self.tune_open, self.show_pane) = (true, true);
        cx.notify();
    }

    fn apply_tune(&mut self, f: Change, same: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.doc.read(cx).doc().selection();
        if selected.is_empty() {
            window.push_notification(Notification::warning("请选中字符后使用"), cx);
            return;
        }
        self.doc.update(cx, |d, cx| d.tune(f, same, cx));
    }

    /// The 字符微调 card at the top of the task pane.
    pub(crate) fn tune_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let doc = self.doc.read(cx).doc();
        let sel = doc.selection();
        let label: String = doc.chars()[sel.clone()].iter().take(6).collect();
        let has = !sel.is_empty();
        let deform_on = cx.global::<AppState>().settings.style.deformation.is_active();
        let btn = |id: &'static str, label: &'static str, tip: &'static str, f: Change| {
            Button::new(id).small().label(label).tooltip(tip).disabled(!has).on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.apply_tune(f, false, w, cx)))
        };
        let arrow = |id: &'static str, icon: IconName, tip: &'static str, f: Change| {
            Button::new(id).small().icon(Icon::new(icon)).tooltip(tip).disabled(!has).on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.apply_tune(f, false, w, cx)))
        };
        let pad = v_flex()
            .items_center()
            .gap_0p5()
            .child(arrow("tune-up", IconName::ArrowUp, "上移0.2", |t| t.dy_mm -= STEP_MM))
            .child(
                h_flex()
                    .gap_0p5()
                    .child(arrow("tune-left", IconName::ArrowLeft, "左移0.2", |t| t.dx_mm -= STEP_MM))
                    .child(arrow("tune-reset-pos", IconName::Dot, "回到原位", |t| (t.dx_mm, t.dy_mm) = (0.0, 0.0)))
                    .child(arrow("tune-right", IconName::ArrowRight, "右移0.2", |t| t.dx_mm += STEP_MM)),
            )
            .child(arrow("tune-down", IconName::ArrowDown, "下移0.2", |t| t.dy_mm += STEP_MM));
        v_flex()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("字符微调"))
                    .child(Button::new("tune-close").ghost().xsmall().label("关闭").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.tune_open = false;
                        cx.notify();
                    }))),
            )
            .child(h_flex().gap_1().text_sm().child("字符：").child(div().font_weight(FontWeight::SEMIBOLD).child(if has { label } else { "未选中".into() })))
            .when(!has, |el| el.child(div().text_xs().text_color(muted).child("请选中字符后使用")))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(pad)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(h_flex().gap_1().child(btn("tune-bigger", "A+", "增大字号5%", |t| t.scale *= 1.0 + STEP_PCT)).child(btn("tune-smaller", "A−", "缩小字号5%", |t| t.scale *= 1.0 - STEP_PCT)))
                            .child(h_flex().gap_1().child(btn("tune-wider", "变胖", "宽度 +5%", |t| t.width *= 1.0 + STEP_PCT)).child(btn("tune-narrower", "变瘦", "宽度 −5%", |t| t.width *= 1.0 - STEP_PCT)))
                            .child(
                                Button::new("tune-reroll")
                                    .small()
                                    .label("变形")
                                    .tooltip(if deform_on { "换一种变形" } else { "请先在设置窗口中启用变形配置" })
                                    .disabled(!has || !deform_on)
                                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.apply_tune(|t| t.reroll = t.reroll.wrapping_add(1), false, w, cx))),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("tune-reset").small().ghost().label("恢复默认").disabled(!has).on_click(cx.listener(|t, _: &ClickEvent, w, cx| {
                        t.apply_tune(|t| *t = Tuning::default(), false, w, cx)
                    })))
                    .child(div().flex_1())
                    .child(
                        Button::new("tune-same")
                            .small()
                            .label("应用至同字符")
                            .tooltip("把选中字符现在的微调复制给文中所有相同的字")
                            .disabled(!has)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.copy_tune_to_same(w, cx))),
                    ),
            )
            .into_any_element()
    }

    /// 应用至同字符: give every equal character the first selected character's tuning.
    fn copy_tune_to_same(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sel, tunings) = {
            let d = self.doc.read(cx).doc();
            (d.selection(), d.tunings())
        };
        if sel.is_empty() {
            window.push_notification(Notification::warning("请选中字符后使用"), cx);
            return;
        }
        let t = tunings[sel.start].unwrap_or_default();
        self.doc.update(cx, |d, cx| d.set_tune_same(t, cx));
    }
}
