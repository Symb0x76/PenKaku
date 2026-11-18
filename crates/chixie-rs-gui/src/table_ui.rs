//! Table forms in the editor: the 内容溢出 dialog for one cell and 批量溢出处理 for all of them.
//! Overflow policies live on the template's cells, so a choice applies to every copy of the page.

use chixie_rs_core::layout::{CellInfo, Overflow};
use chixie_rs_template::{Template, TemplateKind};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::doc_view::DocView;
use crate::state::AppState;

/// The template cell behind a laid-out cell: (template page, table, cell).
pub fn template_cell(t: &Template, page: usize, cell: usize) -> Option<(usize, usize, usize)> {
    let TemplateKind::Table(tt) = &t.kind else { return None };
    let k = page % tt.pages.len().max(1);
    let (table, c) = *tt.cells_in_order(k).get(cell)?;
    Some((k, table, c))
}

/// `t` with one cell's overflow policy changed.
fn with_overflow(t: &Template, page: usize, cell: usize, o: Overflow) -> Option<Template> {
    let (k, table, c) = template_cell(t, page, cell)?;
    let mut t = t.clone();
    if let TemplateKind::Table(tt) = &mut t.kind {
        tt.pages[k].tables[table].cells[c].overflow = o;
    }
    Some(t)
}

/// Store `t` as the active template and relayout the document.
fn commit(t: Template, view: &Entity<DocView>, cx: &mut App) {
    let st = cx.global_mut::<AppState>();
    st.templates.upsert(t);
    st.save_templates();
    view.update(cx, |v, cx| v.relayout_now(cx));
}

/// The 内容溢出 dialog for one overflowing cell: what does not fit, and ChiXie3's three plans
/// with the size / rows each would give.
pub fn open_overflow(view: Entity<DocView>, info: CellInfo, window: &mut Window, cx: &mut App) {
    let Some(t) = cx.global::<AppState>().active_template() else { return };
    let Some(from) = info.overflow_from else { return };
    let text: String = view.read(cx).doc_chars()[from..info.end].iter().collect();
    // What each plan would give, from a trial layout.
    let trial = |o: Overflow, cx: &App| with_overflow(&t, info.page, info.cell, o).and_then(|t2| view.read(cx).trial_cell(&t2, info.seg, cx));
    let shrink = trial(Overflow::Shrink { min: 0.05 }, cx);
    let rows = trial(Overflow::AddRows { max: info.fit.rows.max(1) * 6 }, cx);
    let pct = |s: f32| format!("{:.0}%", s * 100.0);
    let plan1 = shrink.map_or("放不下".to_string(), |c| pct(c.fit.scale));
    let (best_rows, rows_scale) = rows.map_or((info.fit.rows, info.fit.scale), |c| (c.fit.rows, c.fit.scale));
    let rows_note = if best_rows <= info.fit.rows { "（当前行数已是最优行数，方案2不推荐）" } else { "" };
    let (t1, t2, t3) = (t.clone(), t.clone(), t.clone());
    let (v1, v2, v3, v4) = (view.clone(), view.clone(), view.clone(), view.clone());
    let current = pct(info.fit.scale);
    window.open_dialog(cx, move |d, _w, cx| {
        let muted = cx.theme().muted_foreground;
        let (t1, t2, t3) = (t1.clone(), t2.clone(), t3.clone());
        let (v1, v2, v3, v4) = (v1.clone(), v2.clone(), v3.clone(), v4.clone());
        let plan = |title: &'static str, body: String, button: Button| {
            v_flex().gap_1().p_2().rounded_md().border_1().border_color(cx.theme().border).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title)).child(
                h_flex().justify_between().gap_2().child(div().text_sm().text_color(muted).child(body)).child(button),
            )
        };
        d.title("内容溢出").w(px(520.)).child(
            v_flex()
                .gap_2()
                .child(div().text_sm().child("溢出内容："))
                .child(div().p_2().rounded_md().bg(cx.theme().muted).text_sm().child(text.clone()))
                .child(plan(
                    "方案1（仅减小字符尺寸）",
                    format!("当前尺寸：{current}　预期缩至：{plan1}"),
                    Button::new("ov-1").small().primary().label("应用").on_click(move |_, w, cx| {
                        if let Some(t) = with_overflow(&t1, info.page, info.cell, Overflow::Shrink { min: 0.05 }) {
                            commit(t, &v1, cx);
                        }
                        w.close_dialog(cx);
                    }),
                ))
                .child(plan(
                    "方案2（增加单元格行数）",
                    format!("当前行数：{}　最优行数：{best_rows}　需缩放字符：{}{rows_note}", info.fit.rows, pct(rows_scale)),
                    Button::new("ov-2").small().label("应用").on_click(move |_, w, cx| {
                        if let Some(t) = with_overflow(&t2, info.page, info.cell, Overflow::AddRows { max: best_rows }) {
                            commit(t, &v2, cx);
                        }
                        w.close_dialog(cx);
                    }),
                ))
                .child(plan(
                    "方案3（忽略溢出内容）",
                    "溢出的文字不书写；也可直接清空。".to_string(),
                    Button::new("ov-3").small().label("忽略").on_click(move |_, w, cx| {
                        if let Some(t) = with_overflow(&t3, info.page, info.cell, Overflow::Clip) {
                            commit(t, &v3, cx);
                        }
                        w.close_dialog(cx);
                    }),
                ))
                .child(h_flex().justify_end().child(Button::new("ov-clear").small().ghost().label("清空溢出内容").on_click(move |_, w, cx| {
                    v4.update(cx, |v, cx| v.delete_range(from..info.end, cx));
                    w.close_dialog(cx);
                }))),
        )
    });
}

/// 批量溢出处理: one plan for every overflowing cell.
pub fn open_batch_overflow(view: Entity<DocView>, window: &mut Window, cx: &mut App) {
    let count = view.read(cx).layout_doc().overflowing().count();
    let choice = std::rc::Rc::new(std::cell::Cell::new(0usize));
    window.open_dialog(cx, move |d, _w, cx| {
        let muted = cx.theme().muted_foreground;
        let (pick, run) = (choice.clone(), choice.clone());
        let view = view.clone();
        d.title("批量溢出处理").w(px(420.)).child(
            v_flex()
                .gap_2()
                .child(div().text_xs().text_color(muted).child("点击溢出单元格左上角的警告标志，即可处理单个单元格溢出"))
                .child(
                    RadioGroup::vertical("ov-batch")
                        .children(["优先缩放字符", "优先增加内部行数", "优先忽略溢出内容"])
                        .selected_index(Some(pick.get()))
                        .on_click(move |ix, _w, cx| {
                            pick.set(*ix);
                            cx.refresh_windows();
                        }),
                )
                .child(div().text_sm().child(format!("当前溢出单元格：{count} 个")))
                .child(h_flex().justify_end().child(Button::new("ov-run").small().primary().label("开始处理").on_click(move |_, w, cx| {
                    let n = batch_overflow(&view, run.get(), cx);
                    let msg = if n == 0 { "没有单元格内容溢出".to_string() } else { format!("已处理 {n} 个溢出单元格") };
                    w.push_notification(Notification::success(msg), cx);
                    w.close_dialog(cx);
                }))),
        )
    });
}

/// 刷新数据(本页 / 所有页): data-source cells get fresh values (a new draw each time, so it is
/// also 换一组). One undo step.
pub fn refresh_data(view: &Entity<DocView>, all: bool, window: &mut Window, cx: &mut App) {
    let Some(TemplateKind::Table(tt)) = cx.global::<AppState>().active_template().map(|t| t.kind) else { return };
    let (edits, n) = {
        let v = view.read(cx);
        let chars = v.doc_chars();
        let pages = if all {
            0..tt.page_count(chars)
        } else {
            let caret = v.caret_index();
            let seg = chars[..caret.min(chars.len())].iter().filter(|&&c| c == chixie_rs_core::layout::CELL_BREAK).count();
            let j = tt.page_of_segment(seg);
            j..j + 1
        };
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let n = pages.len();
        (tt.refresh(chars, pages, seed), n)
    };
    if edits.is_empty() {
        window.push_notification(Notification::info("没有设置数据源的单元格（模板管理 › 单元格 › 数据源）"), cx);
        return;
    }
    view.update(cx, |v, cx| v.replace_many(edits, cx));
    window.push_notification(Notification::success(format!("已刷新 {n} 页的数据")), cx);
}

/// Apply plan `which` (0 shrink, 1 add rows, 2 ignore) to every overflowing cell; returns how many.
pub fn batch_overflow(view: &Entity<DocView>, which: usize, cx: &mut App) -> usize {
    let Some(mut t) = cx.global::<AppState>().active_template() else { return 0 };
    let cells: Vec<CellInfo> = view.read(cx).layout_doc().overflowing().copied().collect();
    for c in &cells {
        let o = match which {
            0 => Overflow::Shrink { min: 0.05 },
            1 => Overflow::AddRows { max: c.fit.rows.max(1) * 6 },
            _ => Overflow::Clip,
        };
        if let Some(t2) = with_overflow(&t, c.page, c.cell, o) {
            t = t2;
        }
    }
    if !cells.is_empty() {
        commit(t, view, cx);
    }
    cells.len()
}
