//! 手写效果 ↘ dialog: min/max ranges for spacing, offsets, indent and deformation. Every edit
//! is applied to the settings at once (the page re-lays out live).
//!
//! 中文 / 英文 / 数字 (中英文分开设置, not in ChiXie3): letters and digits can have their own size,
//! baseline, float and deformation on top of the Chinese settings.

use chixie_rs_core::deformation::Deformation;
use chixie_rs_core::handwriting::{ScriptStyle, Span, StyleFile};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::state::AppState;

type Get = fn(&StyleFile) -> Span;
type Set = fn(&mut StyleFile, Span);

/// 中文 rows: (section, label, unit, get, set).
const ROWS: [(&str, &str, &str, Get, Set); 8] = [
    ("排布", "中文字距", "mm", |s| s.handwriting.char_spacing_zh, |s, v| s.handwriting.char_spacing_zh = v),
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

/// Script k: 0 = English letters, 1 = digits.
fn script(s: &StyleFile, k: usize) -> &ScriptStyle {
    if k == 0 { &s.handwriting.english } else { &s.handwriting.number }
}

fn script_mut(s: &mut StyleFile, k: usize) -> &mut ScriptStyle {
    if k == 0 { &mut s.handwriting.english } else { &mut s.handwriting.number }
}

/// The script's own deformation, created from the Chinese one on first edit.
fn own_deform(s: &mut StyleFile, k: usize) -> &mut Deformation {
    let base = s.deformation.clone();
    script_mut(s, k).deformation.get_or_insert(base)
}

type SGet = fn(&StyleFile, usize) -> Span;
type SSet = fn(&mut StyleFile, usize, Span);

/// 英文 / 数字 range rows: (label, unit, get, set). Index 0 is 上下浮动, 1–3 the deformation.
const SCRIPT_ROWS: [(&str, &str, SGet, SSet); 4] = [
    ("上下浮动", "mm", |s, k| script(s, k).vertical_offset.unwrap_or(s.handwriting.vertical_offset), |s, k, v| script_mut(s, k).vertical_offset = Some(v)),
    ("横向缩放", "%", |s, k| script(s, k).deformation.as_ref().unwrap_or(&s.deformation).h_scale_pct, |s, k, v| own_deform(s, k).h_scale_pct = v),
    ("纵向缩放", "%", |s, k| script(s, k).deformation.as_ref().unwrap_or(&s.deformation).v_scale_pct, |s, k, v| own_deform(s, k).v_scale_pct = v),
    ("旋转角度", "°", |s, k| script(s, k).deformation.as_ref().unwrap_or(&s.deformation).rotation_deg, |s, k, v| own_deform(s, k).rotation_deg = v),
];

type Pair = (Entity<InputState>, Entity<InputState>);

/// Which input changed.
#[derive(Clone, Copy)]
enum Field {
    Zh(usize),
    EnSpacing,
    Scale(usize),
    Baseline(usize),
    Script(usize, usize),
}

pub struct StyleDialog {
    tab: usize,
    inputs: Vec<Pair>,
    /// Per script: 大小 %, 基线偏移 mm.
    singles: [[Entity<InputState>; 2]; 2],
    /// Per script: the [`SCRIPT_ROWS`] ranges.
    script_inputs: [Vec<Pair>; 2],
    /// Per script: 英文字距 (shared by letters and digits; shown on both tabs).
    en_spacing: Pair,
    _subs: Vec<Subscription>,
}

impl StyleDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let style = cx.global::<AppState>().settings.style.clone();
        let mut subs = Vec::new();
        let mk = |v: f32, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).step(0.1).default_value(fmt_num(v)));
        let mut watch = |e: &Entity<InputState>, field: Field, window: &mut Window, cx: &mut Context<Self>| {
            subs.push(cx.subscribe_in(e, window, move |this, _s, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.apply(field, cx);
                }
            }));
        };
        let mut inputs = Vec::new();
        for (_, _, _, get, _) in ROWS.iter() {
            let span = get(&style);
            inputs.push((mk(span.min, window, cx), mk(span.max, window, cx)));
        }
        let en = style.handwriting.char_spacing_en;
        let en_spacing = (mk(en.min, window, cx), mk(en.max, window, cx));
        let singles = [0, 1].map(|k| {
            let s = script(&style, k);
            [mk(s.scale_pct, window, cx), mk(s.baseline_mm, window, cx)]
        });
        let script_inputs = [0, 1].map(|k| {
            SCRIPT_ROWS
                .iter()
                .map(|(_, _, get, _)| {
                    let span = get(&style, k);
                    (mk(span.min, window, cx), mk(span.max, window, cx))
                })
                .collect::<Vec<_>>()
        });
        for (i, (lo, hi)) in inputs.iter().enumerate() {
            watch(lo, Field::Zh(i), window, cx);
            watch(hi, Field::Zh(i), window, cx);
        }
        watch(&en_spacing.0, Field::EnSpacing, window, cx);
        watch(&en_spacing.1, Field::EnSpacing, window, cx);
        for (k, pair) in singles.iter().enumerate() {
            watch(&pair[0], Field::Scale(k), window, cx);
            watch(&pair[1], Field::Baseline(k), window, cx);
        }
        for (k, rows) in script_inputs.iter().enumerate() {
            for (r, (lo, hi)) in rows.iter().enumerate() {
                watch(lo, Field::Script(k, r), window, cx);
                watch(hi, Field::Script(k, r), window, cx);
            }
        }
        Self { tab: 0, inputs, singles, script_inputs, en_spacing, _subs: subs }
    }

    /// Show tab `tab` (0 中文, 1 英文, 2 数字) with 大小 / 基线 re-read from the settings (the
    /// ribbon's 中英文 group changes them too).
    pub fn show_tab(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab.min(2);
        let style = cx.global::<AppState>().settings.style.clone();
        for (k, pair) in self.singles.iter().enumerate() {
            let s = script(&style, k);
            for (e, v) in pair.iter().zip([s.scale_pct, s.baseline_mm]) {
                if e.read(cx).value().trim().parse::<f32>().ok() != Some(v) {
                    e.update(cx, |st, cx| st.set_value(fmt_num(v), window, cx));
                }
            }
        }
        cx.notify();
    }

    /// Write the edited field back into the settings.
    fn apply(&mut self, field: Field, cx: &mut Context<Self>) {
        let num = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().parse::<f32>().ok().filter(|v| v.is_finite());
        let span = |(lo, hi): &Pair, cx: &App| Some((num(lo, cx)?, num(hi, cx)?)).map(|(a, b)| Span { min: a.min(b), max: a.max(b) });
        let mut s = cx.global::<AppState>().settings.style.clone();
        let before = s.clone();
        match field {
            Field::Zh(i) => {
                if let Some(v) = span(&self.inputs[i], cx) {
                    (ROWS[i].4)(&mut s, v);
                }
            }
            Field::EnSpacing => {
                if let Some(v) = span(&self.en_spacing, cx) {
                    s.handwriting.char_spacing_en = v;
                }
            }
            Field::Scale(k) => {
                if let Some(v) = num(&self.singles[k][0], cx) {
                    script_mut(&mut s, k).scale_pct = v.clamp(20.0, 300.0);
                }
            }
            Field::Baseline(k) => {
                if let Some(v) = num(&self.singles[k][1], cx) {
                    script_mut(&mut s, k).baseline_mm = v;
                }
            }
            Field::Script(k, r) => {
                if let Some(v) = span(&self.script_inputs[k][r], cx) {
                    (SCRIPT_ROWS[r].3)(&mut s, k, v);
                }
            }
        }
        if s != before {
            let st = cx.global_mut::<AppState>();
            st.settings.style = s;
            st.save_settings();
        }
    }

    fn range_row(label: &'static str, unit: &'static str, (lo, hi): &Pair, muted: Hsla) -> Div {
        h_flex()
            .gap_2()
            .child(div().w(px(72.)).text_sm().text_color(muted).child(label))
            .child(div().w(px(110.)).child(NumberInput::new(lo).small().suffix(unit)))
            .child(div().text_sm().text_color(muted).child("至"))
            .child(div().w(px(110.)).child(NumberInput::new(hi).small().suffix(unit)))
    }

    fn single_row(label: &'static str, unit: &'static str, e: &Entity<InputState>, hint: &'static str, muted: Hsla) -> Div {
        h_flex()
            .gap_2()
            .child(div().w(px(72.)).text_sm().text_color(muted).child(label))
            .child(div().w(px(110.)).child(NumberInput::new(e).small().suffix(unit)))
            .child(div().text_xs().text_color(muted).child(hint))
    }

    fn chinese_tab(&self, style: &StyleFile, muted: Hsla) -> Div {
        let mut col = v_flex().gap_1p5();
        let mut last = "";
        for (i, (section, label, unit, _, _)) in ROWS.iter().enumerate() {
            if *section != last {
                last = section;
                col = col.child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(*section));
            }
            col = col.child(Self::range_row(label, unit, &self.inputs[i], muted));
        }
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
    }

    /// 英文 (k = 0) or 数字 (k = 1).
    fn script_tab(&self, style: &StyleFile, k: usize, muted: Hsla) -> Div {
        let sc = script(style, k).clone();
        let same_float = sc.vertical_offset.is_none();
        let same_deform = sc.deformation.is_none();
        let own_on = sc.deformation.as_ref().is_some_and(Deformation::is_active);
        let toggle = move |f: fn(&mut StyleFile, usize, bool)| {
            move |v: &bool, _w: &mut Window, cx: &mut App| {
                let st = cx.global_mut::<AppState>();
                f(&mut st.settings.style, k, *v);
                st.save_settings();
            }
        };
        let section = |t: &'static str| div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child(t);
        let rows = &self.script_inputs[k];
        let mut col = v_flex()
            .gap_1p5()
            .child(section("大小与位置"))
            .child(Self::single_row("大小", "%", &self.singles[k][0], "相对中文字号；字距随之缩放", muted))
            .child(Self::single_row("基线偏移", "mm", &self.singles[k][1], "正数上移", muted))
            .child(Self::range_row("英文字距", "mm", &self.en_spacing, muted))
            .child(section("上下浮动"))
            .child(Checkbox::new(("same-float", k)).small().label("同中文").checked(same_float).on_click(toggle(|s, k, v| {
                let main = s.handwriting.vertical_offset;
                script_mut(s, k).vertical_offset = (!v).then_some(main);
            })))
            .when(!same_float, |el| el.child(Self::range_row(SCRIPT_ROWS[0].0, SCRIPT_ROWS[0].1, &rows[0], muted)))
            .child(section("字符变形"))
            .child(Checkbox::new(("same-deform", k)).small().label("同中文").checked(same_deform).on_click(toggle(|s, k, v| {
                let main = s.deformation.clone();
                script_mut(s, k).deformation = (!v).then_some(main);
            })));
        if !same_deform {
            col = col.child(Checkbox::new(("own-deform", k)).small().label("启用变形").checked(own_on).on_click(toggle(|s, k, v| {
                let d = own_deform(s, k);
                (d.enabled, d.base) = (v, v || d.base);
            })));
            for (r, (label, unit, _, _)) in SCRIPT_ROWS.iter().enumerate().skip(1) {
                col = col.child(Self::range_row(label, unit, &rows[r], muted));
            }
        }
        col
    }
}

fn fmt_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Render for StyleDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let style = cx.global::<AppState>().settings.style.clone();
        let body = match self.tab {
            0 => self.chinese_tab(&style, muted),
            k => self.script_tab(&style, k - 1, muted),
        };
        v_flex()
            .gap_2()
            .child(
                TabBar::new("script-tabs")
                    .small()
                    .segmented()
                    .selected_index(self.tab)
                    .children(["中文", "英文", "数字"].map(|l| Tab::new().label(l)))
                    .on_click(cx.listener(|t, ix: &usize, _w, cx| {
                        t.tab = *ix;
                        cx.notify();
                    })),
            )
            .child(body)
            .child(div().pt_2().text_xs().text_color(muted).child("修改即时生效；总开关在「手写」选项卡。英文、数字未单独设置的项沿用中文。范围数值需重新打开本窗口才会刷新。"))
    }
}
