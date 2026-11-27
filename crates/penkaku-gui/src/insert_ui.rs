//! 插入 表格 / 流程图: the ribbon groups and their dialogs. Tables and flowcharts sit in the text
//! and the pen draws their lines (see `penkaku_core::layout::Block`).

use penkaku_core::diagram::mermaid;
use penkaku_doc::edit::TABLE_WIDTH;
use penkaku_doc::model::Block;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputState, NumberInput, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::doc_view::DocView;
use crate::ribbon::{big, group, small, stack, toggle};
use crate::state::AppState;

/// What 流程图 starts with: a small example to edit.
pub const SAMPLE_FLOWCHART: &str = "flowchart TD\n    A[开始] --> B{条件成立?}\n    B -->|是| C[执行]\n    B -->|否| D[结束]\n    C --> D";

/// Writable width of the page (mm), for a new table's width.
fn page_width(cx: &App) -> f32 {
    let (w, _) = cx.global::<AppState>().settings.page.writable();
    if w.is_finite() && w > 0.0 { w } else { TABLE_WIDTH }
}

/// The 插入 group: 表格 and 流程图.
pub fn insert_group(view: Entity<DocView>, cx: &App) -> AnyElement {
    let v2 = view.clone();
    group(
        cx,
        "insert",
        "插入",
        h_flex()
            .gap_1()
            .child(
                big("ins-table", "表格", IconName::Table)
                    .tooltip("插入由笔画出格线的表格，单元格内直接输入文字（Tab 到下一格）")
                    .on_click(move |_: &ClickEvent, w, cx| open_table_dialog(view.clone(), w, cx)),
            )
            .child(
                big("ins-flow", "流程图", IconName::Workflow)
                    .tooltip("用 Mermaid 语法插入流程图，由笔画出图框、箭头和文字")
                    .on_click(move |_: &ClickEvent, w, cx| open_diagram_dialog(v2.clone(), None, w, cx)),
            ),
        None,
    )
    .into_any_element()
}

/// Contextual groups for the caret's object: 表格工具 inside an inline table, 流程图 next to a
/// flowchart. `None` elsewhere.
pub fn object_tools(view: Entity<DocView>, cx: &App) -> Option<AnyElement> {
    let tint = cx.theme().primary.opacity(0.06);
    let v = view.read(cx);
    if let Some(t) = v.inline_table() {
        let run = |f: fn(&mut penkaku_doc::EditDoc) -> bool| {
            let view = view.clone();
            move |_: &ClickEvent, w: &mut Window, cx: &mut App| {
                let focus = view.read(cx).focus_handle(cx);
                view.update(cx, |v, cx| v.edit_doc(f, cx));
                w.focus(&focus, cx);
            }
        };
        let body = h_flex()
            .gap_1()
            .child(stack([
                small("it-next", "下一格 (Tab)", IconName::ArrowRight).on_click(run(|d| d.next_inline_cell())).into_any_element(),
                small("it-prev", "上一格 (Shift+Tab)", IconName::ArrowLeft).on_click(run(|d| d.prev_inline_cell())).into_any_element(),
            ]))
            .child(stack([
                small("it-add-row", "在下方插入行", IconName::BetweenHorizontalEnd).on_click(run(|d| d.add_row())).into_any_element(),
                small("it-del-row", "删除行", IconName::Rows3).on_click(run(|d| d.delete_row())).into_any_element(),
            ]))
            .child(stack([
                small("it-add-col", "在右侧插入列", IconName::BetweenVerticalEnd).on_click(run(|d| d.add_col())).into_any_element(),
                small("it-del-col", "删除列", IconName::Columns2).on_click(run(|d| d.delete_col())).into_any_element(),
            ]))
            .child(stack([
                toggle("it-borders", "框线", IconName::Table, t.framed, "笔是否画出这张表格的格线（手写一般不画框；含图片或只有一格的表格默认不画，纯文字表格默认画）")
                    .on_click(run(|d| d.toggle_table_borders()))
                    .into_any_element(),
                small("it-del", "删除表格", IconName::Grid2x2X).on_click(run(|d| d.delete_table())).into_any_element(),
            ]));
        return Some(group(cx, "inline-table", "表格工具", body, None).bg(tint).into_any_element());
    }
    let (at, source) = v.diagram_at_caret()?;
    let body = stack([small("fc-edit", "编辑流程图…", IconName::SquarePen)
        .on_click(move |_: &ClickEvent, w, cx| open_diagram_dialog(view.clone(), Some((at, source.clone())), w, cx))
        .into_any_element()]);
    Some(group(cx, "flowchart", "流程图", body, None).bg(tint).into_any_element())
}

/// 插入表格: rows, columns and total width.
pub fn open_table_dialog(view: Entity<DocView>, window: &mut Window, cx: &mut App) {
    let width = page_width(cx).min(TABLE_WIDTH).round();
    let rows = cx.new(|cx| InputState::new(window, cx).step(1.0).min(1.0).max(50.0).default_value("3"));
    let cols = cx.new(|cx| InputState::new(window, cx).step(1.0).min(1.0).max(10.0).default_value("3"));
    let wide = cx.new(|cx| InputState::new(window, cx).step(5.0).min(10.0).max(1000.0).default_value(format!("{width}")));
    window.open_dialog(cx, move |d, _w, cx| {
        let muted = cx.theme().muted_foreground;
        let field = |label: &'static str, st: &Entity<InputState>, suffix: Option<&'static str>| {
            let n = NumberInput::new(st).small();
            h_flex().gap_2().items_center().child(div().w(px(56.)).text_sm().child(label)).child(div().flex_1().child(match suffix {
                Some(s) => n.suffix(s),
                None => n,
            }))
        };
        let (view, rows_s, cols_s, wide_s) = (view.clone(), rows.clone(), cols.clone(), wide.clone());
        let ok = Button::new("tbl-ok").small().primary().label("插入").on_click(move |_: &ClickEvent, w, cx| {
            let num = |s: &Entity<InputState>, lo: f32, hi: f32| s.read(cx).value().trim().parse::<f32>().ok().filter(|v| (lo..=hi).contains(v));
            let (Some(r), Some(c), Some(wd)) = (num(&rows_s, 1.0, 50.0), num(&cols_s, 1.0, 10.0), num(&wide_s, 10.0, 1000.0)) else {
                w.push_notification(Notification::warning("行数 1–50，列数 1–10，宽度 10–1000 mm"), cx);
                return;
            };
            view.update(cx, |v, cx| v.edit_doc(|d| {
                d.insert_table(r as usize, c as usize, wd);
                true
            }, cx));
            w.close_dialog(cx);
            let focus = view.read(cx).focus_handle(cx);
            w.focus(&focus, cx);
        });
        let cancel = Button::new("tbl-cancel").small().label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx));
        d.title("插入表格")
            .w(px(360.))
            .child(
                v_flex()
                    .gap_2()
                    .child(field("行数", &rows, None))
                    .child(field("列数", &cols, None))
                    .child(field("总宽度", &wide, Some("mm")))
                    .child(div().text_xs().text_color(muted).child("各列等宽；表格线和文字都由笔写出，跟随「手写」效果。")),
            )
            .footer(h_flex().gap_2().justify_end().child(cancel).child(ok))
    });
}

/// 插入流程图 / 编辑流程图: Mermaid source, checked before it goes in. `edit`: the flowchart
/// being edited (anchor index, current source).
pub fn open_diagram_dialog(view: Entity<DocView>, edit: Option<(usize, String)>, window: &mut Window, cx: &mut App) {
    let text = cx.new(|cx| TextareaState::new(window, cx).auto_grow(8, 18));
    let initial = edit.as_ref().map_or(SAMPLE_FLOWCHART.to_string(), |e| e.1.clone());
    text.update(cx, |s, cx| s.set_value(initial, window, cx));
    let title = if edit.is_some() { "编辑流程图" } else { "插入流程图" };
    window.open_dialog(cx, move |d, _w, cx| {
        let muted = cx.theme().muted_foreground;
        let (view, text_s) = (view.clone(), text.clone());
        let at = edit.as_ref().map(|e| e.0);
        let ok = Button::new("fc-ok").small().primary().label(if at.is_some() { "确定" } else { "插入" }).on_click(move |_: &ClickEvent, w, cx| {
            let source = text_s.read(cx).value().to_string();
            if let Err(e) = mermaid::parse(&source) {
                w.push_notification(Notification::warning(format!("流程图有误：{e}")), cx);
                return;
            }
            view.update(cx, |v, cx| v.edit_doc(|d| {
                match at {
                    Some(k) => d.set_block(k, Block::Diagram { source }),
                    None => d.insert_diagram(&source),
                }
                true
            }, cx));
            w.close_dialog(cx);
            let focus = view.read(cx).focus_handle(cx);
            w.focus(&focus, cx);
        });
        let cancel = Button::new("fc-cancel").small().label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx));
        d.title(title)
            .w(px(520.))
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_xs().text_color(muted).child(
                        "Mermaid 流程图语法：第一行 flowchart TD（自上而下）或 LR（自左而右）；节点 A[方框] B(圆角) C{判断} D((圆))；连线 -->、---、-.->，连线文字 -->|文字|。",
                    ))
                    .child(Textarea::new(&text)),
            )
            .footer(h_flex().gap_2().justify_end().child(cancel).child(ok))
    });
}
