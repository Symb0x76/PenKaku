//! 字符微调, laid out like ChiXie3's dialog: the selected character with a preview, 字体, 字号 with
//! A+ / A−, the 微调 pad next to 变形 / 变瘦 / 变胖, then 恢复默认 and 应用至同字符 ▾, and the two red
//! font clean-ups. Every click is one undoable edit.

use penkaku_core::font::fix::FontFix;
use penkaku_core::layout::{Stroke, Tuning};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::PenKakuApp;
use crate::state::AppState;

/// One click of the arrow pad (ChiXie3: 上移0.2 …).
pub(crate) const STEP_MM: f32 = 0.2;
/// A+ / A− (ChiXie3: 增大字号5%) and 变胖 / 变瘦.
pub(crate) const STEP_PCT: f32 = 0.05;
/// Side of the preview box, px.
const PREVIEW_PX: f32 = 64.0;

type Change = fn(&mut Tuning);

impl PenKakuApp {
    pub(crate) fn open_tune(&mut self, cx: &mut Context<Self>) {
        (self.pane_mode, self.show_pane) = (super::PaneMode::Tune, true);
        cx.notify();
    }

    /// True when a character is selected; else the ChiXie3 warning.
    fn tune_ready(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let has = !self.doc.read(cx).doc().selection().is_empty();
        if !has {
            window.push_notification(Notification::warning("请选中字符后使用"), cx);
        }
        has
    }

    fn apply_tune(&mut self, f: Change, window: &mut Window, cx: &mut Context<Self>) {
        if self.tune_ready(window, cx) {
            self.doc.update(cx, |d, cx| d.tune(f, false, cx));
        }
    }

    /// 字符重复轨迹优化 / 异常宽度字符处理: the clean-up for the whole font the selected char uses.
    fn fix_selected_font(&mut self, add: FontFix, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tune_ready(window, cx) {
            return;
        }
        match self.doc.read(cx).selected_font_key(cx) {
            Some(key) => {
                self.add_font_fix(key, add, window, cx);
                window.push_notification(Notification::success("已对该字符所在的字体完成优化"), cx);
            }
            None => window.push_notification(Notification::warning("找不到这个字符所用的字体"), cx),
        }
    }

    /// The 字符微调 card at the top of the task pane.
    pub(crate) fn tune_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, ink, danger, muted_bg) = (theme.muted_foreground, theme.border, theme.foreground, theme.danger, theme.muted);
        let view = self.doc.read(cx);
        let doc = view.doc();
        let sel = doc.selection();
        let has = !sel.is_empty();
        let label: String = doc.chars()[sel.clone()].iter().take(6).collect();
        let fs = view.format_state(cx);
        let strokes = if has { view.selected_strokes() } else { Vec::new() };
        let deform_on = cx.global::<AppState>().settings.style.deformation.is_active();
        let entity = self.doc.clone();

        let tune_btn = |id: &'static str, label: &'static str, tip: &'static str, f: Change| {
            Button::new(id).small().label(label).tooltip(tip).disabled(!has).on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.apply_tune(f, w, cx)))
        };
        let arrow = |id: &'static str, icon: IconName, tip: &'static str, f: Change| {
            Button::new(id).small().ghost().icon(Icon::new(icon)).tooltip(tip).disabled(!has).on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.apply_tune(f, w, cx)))
        };
        // 54 px fits a two-character label + colon at text_sm on one line.
        let row = |name: &'static str| h_flex().gap_2().items_center().child(div().w(px(54.)).flex_shrink_0().whitespace_nowrap().text_sm().child(name));

        // 字体 and 字号 set the selected chars' own font and size, like the ribbon.
        let font_label = match &fs.font {
            Some(Some(name)) => name.clone(),
            Some(None) => "文档字体".to_string(),
            None => "（混合）".to_string(),
        };
        let e = entity.clone();
        let font_btn = Button::new("tune-font").small().outline().label(font_label).dropdown_caret(true).disabled(!has).dropdown_menu(move |m, _w, cx| {
            let st = cx.global::<AppState>();
            let main = st.active_font.as_ref().map(|f| f.name.clone());
            let e0 = e.clone();
            let m = m.item(PopupMenuItem::new("文档字体（默认）").on_click(move |_, _w, cx| e0.update(cx, |d, cx| d.set_run_font(None, cx))));
            st.fonts.list().into_iter().fold(m, |m, f| {
                let e = e.clone();
                let name = (main.as_deref() != Some(f.name.as_str())).then(|| f.name.clone());
                m.item(PopupMenuItem::new(f.name).on_click(move |_, _w, cx| e.update(cx, |d, cx| d.set_run_font(name.clone(), cx))))
            })
        });
        let e = entity.clone();
        let size_btn = Button::new("tune-size")
            .small()
            .outline()
            .label(fs.size.map_or("字号".to_string(), super::fmt_num))
            .dropdown_caret(true)
            .disabled(!has)
            .dropdown_menu(move |m, _w, _cx| {
                crate::doc_view::SIZES.iter().fold(m, |m, &s| {
                    let e = e.clone();
                    m.item(PopupMenuItem::new(super::fmt_num(s)).on_click(move |_, _w, cx| e.update(cx, |d, cx| d.set_run_size(s, cx))))
                })
            });
        let e = entity.clone();
        let weight = doc.common(|a| a.weight);
        let weight_btn = Button::new("tune-weight")
            .small()
            .outline()
            .label(weight.map_or("（混合）", weight_label))
            .dropdown_caret(true)
            .disabled(!has)
            .tooltip("落笔轻重：步进/舵机抬笔时按「轻重每级」调整落笔深度")
            .dropdown_menu(move |m, _w, _cx| {
                (-2..=2).rev().fold(m, |m, w: i8| {
                    let e = e.clone();
                    m.item(PopupMenuItem::new(weight_label(w)).checked(weight == Some(w)).on_click(move |_, _w, cx| e.update(cx, |d, cx| d.set_run_weight(w, cx))))
                })
            });
        let bump = |id: &'static str, label: &'static str, tip: &'static str, factor: f32| {
            let e = entity.clone();
            Button::new(id).small().ghost().label(label).tooltip(tip).disabled(!has).on_click(move |_: &ClickEvent, _w, cx| e.update(cx, |d, cx| d.scale_run_size(factor, cx)))
        };

        let pad = v_flex()
            .items_center()
            .child(arrow("tune-up", IconName::ArrowUp, "上移0.2", |t| t.dy_mm -= STEP_MM))
            .child(
                h_flex()
                    .child(arrow("tune-left", IconName::ArrowLeft, "左移0.2", |t| t.dx_mm -= STEP_MM))
                    .child(arrow("tune-centre", IconName::Dot, "回到原位", |t| (t.dx_mm, t.dy_mm) = (0.0, 0.0)))
                    .child(arrow("tune-right", IconName::ArrowRight, "右移0.2", |t| t.dx_mm += STEP_MM)),
            )
            .child(arrow("tune-down", IconName::ArrowDown, "下移0.2", |t| t.dy_mm += STEP_MM));
        let shape = v_flex()
            .gap_1()
            .child(
                Button::new("tune-reroll")
                    .small()
                    .w(px(72.))
                    .label("变形")
                    .tooltip(if deform_on { "换一种变形" } else { "请先在设置窗口中启用变形配置" })
                    .disabled(!has || !deform_on)
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.apply_tune(|t| t.reroll = t.reroll.wrapping_add(1), w, cx))),
            )
            .child(tune_btn("tune-narrower", "变瘦", "宽度 −5%", |t| t.width *= 1.0 - STEP_PCT).w(px(72.)))
            .child(tune_btn("tune-wider", "变胖", "宽度 +5%", |t| t.width *= 1.0 + STEP_PCT).w(px(72.)));

        let same = DropdownButton::new("tune-same")
            .small()
            .disabled(!has)
            .button(Button::new("tune-same-main").label("应用至同字符").tooltip("把选中字符的字体、字号、轻重和微调用到文中所有相同的字").on_click(cx.listener(
                |t, _: &ClickEvent, w, cx| {
                    if t.tune_ready(w, cx) {
                        t.doc.update(cx, |d, cx| d.apply_tune_to_same(false, cx));
                    }
                },
            )))
            .dropdown_menu({
                let e = entity.clone();
                move |m, _w, _cx| {
                    let (e1, e2) = (e.clone(), e.clone());
                    m.item(PopupMenuItem::new("应用至同字符").on_click(move |_, _w, cx| e1.update(cx, |d, cx| d.apply_tune_to_same(false, cx))))
                        .item(PopupMenuItem::new("应用至同字符(同时变形)").on_click(move |_, _w, cx| e2.update(cx, |d, cx| d.apply_tune_to_same(true, cx))))
                }
            });

        let red = |id: &'static str, label: &'static str, tip: &'static str, add: FontFix| {
            Button::new(id).small().ghost().text_color(danger).label(label).tooltip(tip).disabled(!has).on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.fix_selected_font(add, w, cx)))
        };

        let fix_open = self.tune_fix_open;
        // Whole-font clean-ups, folded away so they don't read as per-character actions.
        let font_fixes = v_flex()
            .gap_1()
            .pt_2()
            .border_t_1()
            .border_color(border)
            .child(
                Button::new("tune-fix-toggle")
                    .xsmall()
                    .ghost()
                    .justify_start()
                    .icon(Icon::new(if fix_open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)))
                    .label("全局字体修复（作用于整套字体）")
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.tune_fix_open ^= true;
                        cx.notify();
                    })),
            )
            .when(fix_open, |el| {
                el.child(div().text_xs().text_color(muted).child("以下操作修改选中字符所在的整套字体，不只是这个字。"))
                    .child(
                        v_flex()
                            .items_start()
                            .child(red("tune-dedupe", "字符重复轨迹优化", "对选中字符所在的字体进行整个的重复轨迹优化", FontFix { dedupe: true, ..Default::default() }))
                            .child(red("tune-widths", "异常宽度字符处理", "对选中字符所在的字体处理宽度异常的字符", FontFix { widths: true, ..Default::default() })),
                    )
            });

        let controls = v_flex()
            .gap_2()
            // Nothing selected: the controls fade back behind the hint.
            .when(!has, |el| el.opacity(0.55))
            .child(row("字体：").child(font_btn))
            .child(row("字号：").child(size_btn).child(bump("tune-bigger", "A+", "增大字号5%", 1.0 + STEP_PCT)).child(bump("tune-smaller", "A−", "缩小字号5%", 1.0 - STEP_PCT)))
            .child(row("轻重：").child(weight_btn))
            .child(row("微调：").child(pad).child(div().flex_1()).child(shape))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("tune-reset").small().ghost().label("恢复默认").tooltip("当前版本恢复默认只可恢复变形、变胖、变瘦方面变动").disabled(!has).on_click(
                        cx.listener(|t, _: &ClickEvent, w, cx| {
                            t.apply_tune(
                                |t| {
                                    (t.width, t.scale, t.reroll) = (1.0, 1.0, 0);
                                },
                                w,
                                cx,
                            )
                        }),
                    ))
                    .child(div().flex_1())
                    .child(same),
            );

        v_flex()
            .gap_2()
            .p_3()
            .child(if has {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(h_flex().flex_1().gap_1().text_sm().child("字符：").child(div().font_weight(FontWeight::SEMIBOLD).child(label)))
                    .child(div().size(px(PREVIEW_PX)).flex_shrink_0().rounded_md().border_1().border_color(border).child(glyph_preview(strokes, ink)))
                    .into_any_element()
            } else {
                div().p_2().rounded_md().bg(muted_bg).text_sm().child("在文档中拖选 1 个或多个字符，即可微调位置、大小与轻重。").into_any_element()
            })
            .child(controls)
            .child(font_fixes)
            .into_any_element()
    }
}

/// The selected character as laid out (with its 微调 and 变形), fitted into the preview box.
/// 落笔轻重 names, as in ChiXie3's N1 settings.
fn weight_label(w: i8) -> &'static str {
    match w {
        ..=-2 => "更轻Ⅱ",
        -1 => "更轻Ⅰ",
        0 => "标准(默认)",
        1 => "更重Ⅰ",
        _ => "更重Ⅱ",
    }
}

fn glyph_preview(strokes: Vec<Stroke>, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let pts = strokes.iter().flatten();
            let (x0, x1) = pts.clone().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (y0, y1) = pts.fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            if x0 > x1 {
                return;
            }
            let side = f32::from(bounds.size.width).min(f32::from(bounds.size.height)) * 0.8;
            let s = side / (x1 - x0).max(y1 - y0).max(0.1);
            let (cx0, cy0) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let mid = bounds.center();
            // Page mm have y up; the screen has y down.
            let at = |(x, y): (f32, f32)| mid + point(px((x - cx0) * s), px((cy0 - y) * s));
            let mut path = PathBuilder::stroke(px(1.5));
            for st in &strokes {
                let Some(&first) = st.first() else { continue };
                path.move_to(at(first));
                if st.len() == 1 {
                    path.line_to(at(first) + point(px(0.8), px(0.8)));
                }
                for &q in &st[1..] {
                    path.line_to(at(q));
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}
