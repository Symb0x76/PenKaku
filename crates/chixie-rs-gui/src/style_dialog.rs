//! 手写效果 ↘ dialog: min/max ranges for spacing, offsets, indent and deformation. Every edit
//! is applied to the settings at once (the page re-lays out live).

use chixie_rs_core::deformation::Deformation;
use chixie_rs_core::handwriting::{Span, StyleFile};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::state::AppState;

type Get = fn(&StyleFile) -> Span;
type Set = fn(&mut StyleFile, Span);

/// (section, label, unit, get, set)
const ROWS: [(&str, &str, &str, Get, Set); 9] = [
    ("排布", "中文字距", "mm", |s| s.handwriting.char_spacing_zh, |s, v| s.handwriting.char_spacing_zh = v),
    ("排布", "英文字距", "mm", |s| s.handwriting.char_spacing_en, |s, v| s.handwriting.char_spacing_en = v),
    ("排布", "空格宽度", "mm", |s| s.handwriting.space_width_zh, |s, v| {
        s.handwriting.space_width_zh = v;
        s.handwriting.space_width_en = v;
    }),
    ("排布", "上下浮动", "mm", |s| s.handwriting.vertical_offset, |s, v| s.handwriting.vertical_offset = v),
    ("排布", "行首偏移", "mm", |s| s.handwriting.line_start_offset, |s, v| s.handwriting.line_start_offset = v),
    ("排布", "首行缩进", "mm", |s| s.handwriting.first_line_indent.unwrap_or(Span { min: 0.0, max: 0.0 }), |s, v| {
        s.handwriting.first_line_indent = (v.max > 0.0).then_some(v)
    }),
    ("变形", "横向缩放", "%", |s| s.deformation.h_scale_pct, |s, v| s.deformation.h_scale_pct = v),
    ("变形", "纵向缩放", "%", |s| s.deformation.v_scale_pct, |s, v| s.deformation.v_scale_pct = v),
    ("变形", "旋转角度", "°", |s| s.deformation.rotation_deg, |s, v| s.deformation.rotation_deg = v),
];

pub struct StyleDialog {
    inputs: Vec<(Entity<InputState>, Entity<InputState>)>,
    _subs: Vec<Subscription>,
}

impl StyleDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let style = cx.global::<AppState>().settings.style.clone();
        let mut inputs = Vec::new();
        let mut subs = Vec::new();
        for (i, (_, _, _, get, _)) in ROWS.iter().enumerate() {
            let span = get(&style);
            let mk = |v: f32, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| InputState::new(window, cx).step(0.1).default_value(fmt_num(v)))
            };
            let (lo, hi) = (mk(span.min, window, cx), mk(span.max, window, cx));
            for e in [&lo, &hi] {
                subs.push(cx.subscribe_in(e, window, move |this, _s, ev: &InputEvent, _w, cx| {
                    if matches!(ev, InputEvent::Change) {
                        this.apply(i, cx);
                    }
                }));
            }
            inputs.push((lo, hi));
        }
        Self { inputs, _subs: subs }
    }

    fn apply(&mut self, i: usize, cx: &mut Context<Self>) {
        let parse = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().parse::<f32>().ok().filter(|v| v.is_finite());
        let (lo, hi) = &self.inputs[i];
        let (Some(a), Some(b)) = (parse(lo, cx), parse(hi, cx)) else { return };
        let span = Span { min: a.min(b), max: a.max(b) };
        let set = ROWS[i].4;
        let st = cx.global_mut::<AppState>();
        set(&mut st.settings.style, span);
        st.save_settings();
    }
}

fn fmt_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Render for StyleDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut col = v_flex().gap_1p5();
        let mut last = "";
        for (i, (section, label, unit, _, _)) in ROWS.iter().enumerate() {
            if *section != last {
                last = section;
                col = col.child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(*section));
            }
            let (lo, hi) = &self.inputs[i];
            col = col.child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(72.)).text_sm().text_color(muted).child(*label))
                    .child(div().w(px(110.)).child(NumberInput::new(lo).small().suffix(*unit)))
                    .child(div().text_sm().text_color(muted).child("至"))
                    .child(div().w(px(110.)).child(NumberInput::new(hi).small().suffix(*unit))),
            );
        }
        let style = cx.global::<AppState>().settings.style.clone();
        let edit = |f: fn(&mut StyleFile, bool)| {
            move |v: &bool, _w: &mut Window, cx: &mut App| {
                let st = cx.global_mut::<AppState>();
                f(&mut st.settings.style, *v);
                st.save_settings();
            }
        };
        let shape = style.deformation.shape_degree;
        let shape_menu = Button::new("shape-degree").small().outline().label(format!("字形变形幅度 {}", fmt_num(shape))).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            (0..=10).fold(m, |m, k| {
                m.item(PopupMenuItem::new(k.to_string()).checked(k as f32 == shape).on_click(move |_, _w, cx| {
                    let st = cx.global_mut::<AppState>();
                    st.settings.style.deformation.shape_degree = k as f32;
                    st.save_settings();
                }))
            })
        });
        col.child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("细节"))
            .child(
                h_flex()
                    .gap_4()
                    .child(Checkbox::new("tails").small().label("随机删除小尾巴").checked(style.deformation.remove_tails).on_click(edit(|s, v| s.deformation.remove_tails = v)))
                    .child(
                        Checkbox::new("drift")
                            .small()
                            .label("智能偏移（同向偏移）")
                            .checked(style.handwriting.intelligent_vertical_offset)
                            .on_click(edit(|s, v| s.handwriting.intelligent_vertical_offset = v)),
                    ),
            )
            .child(
                h_flex().gap_2().child(shape_menu).child(div().flex_1()).child(Button::new("recommended").small().label("使用推荐参数").on_click(|_: &ClickEvent, window, cx| {
                    window.open_dialog(cx, |d, _w, _cx| {
                        d.title("使用推荐参数").w(px(360.)).child("确认将字符变形参数设置为推荐参数吗？").footer(
                            h_flex().gap_2().justify_end().child(Button::new("rec-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx))).child(
                                Button::new("rec-ok").primary().label("确定").on_click(|_: &ClickEvent, w, cx| {
                                    let st = cx.global_mut::<AppState>();
                                    let d = &mut st.settings.style.deformation;
                                    // ChiXie3's shipped values; the on/off switches stay as they are.
                                    let rec = Deformation::recommended();
                                    (d.h_scale_pct, d.v_scale_pct, d.rotation_deg, d.stroke_degree, d.shape_degree, d.remove_tails) =
                                        (rec.h_scale_pct, rec.v_scale_pct, rec.rotation_deg, rec.stroke_degree, rec.shape_degree, rec.remove_tails);
                                    st.save_settings();
                                    w.close_dialog(cx);
                                }),
                            ),
                        )
                    });
                })),
            )
            .child(div().pt_2().text_xs().text_color(muted).child("修改即时生效；总开关在「手写」选项卡。范围数值需重新打开本窗口才会刷新。"))
    }
}
