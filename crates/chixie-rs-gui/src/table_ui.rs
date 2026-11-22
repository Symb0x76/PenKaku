//! Table forms in the editor: the 内容溢出 dialog for one cell and 批量溢出处理 for all of them.
//! Overflow policies live on the template's cells, so a choice applies to every copy of the page.

use chixie_rs_core::layout::{CellInfo, Overflow};
use chixie_rs_template::{Template, TemplateKind};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::tab::{Tab, TabBar};
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

/// 批量导入: 批量导入数据 (xlsx / xls / ods into the form's pages) and 批量名单建页 (one page per
/// line of a list, written into the caret's cell).
pub fn open_batch_import(view: Entity<DocView>, window: &mut Window, cx: &mut App) {
    let Some(TemplateKind::Table(tt)) = cx.global::<AppState>().active_template().map(|t| t.kind) else {
        window.push_notification(Notification::warning("请先应用一个表格模板（布局 › 模板）"), cx);
        return;
    };
    // 目标单元格: the caret's cell, numbered in writing order within its page.
    let target = {
        let v = view.read(cx);
        let chars = v.doc_chars();
        let seg = chars[..v.caret_index().min(chars.len())].iter().filter(|&&c| c == chixie_rs_core::layout::CELL_BREAK).count();
        seg - tt.first_segment(tt.page_of_segment(seg))
    };
    let names = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 12));
    let tab = std::rc::Rc::new(std::cell::Cell::new(0usize));
    window.open_dialog(cx, move |d, _w, cx| {
        let muted = cx.theme().muted_foreground;
        let (view, names, tab, tt) = (view.clone(), names.clone(), tab.clone(), tt.clone());
        let set = tab.clone();
        let body: AnyElement = if tab.get() == 0 {
            let view = view.clone();
            v_flex()
                .gap_2()
                .child(div().text_sm().text_color(muted).child("批量导入数据文件，或导入多页表格实现批量数据导入"))
                .child(div().text_xs().text_color(muted).child("每个工作表写成一页（多页模板需要和模板页数相同的工作表），按「Excel导入配置」的范围对应单元格；合并单元格展开。"))
                .child(Button::new("bi-file").primary().label("选择文件…").on_click(move |_: &ClickEvent, w, cx| {
                    w.close_dialog(cx);
                    import_file(view.clone(), w, cx);
                }))
                .into_any_element()
        } else {
            let label = if tt.pages.len() == 1 { format!("目标单元格：第 {} 格", target + 1) } else { "目标单元格：未选中".to_string() };
            v_flex()
                .gap_2()
                .child(div().text_sm().text_color(muted).child("根据名单数据，自动新建对应数量表格页面并填充"))
                .child(div().text_sm().child(label))
                .child(div().text_xs().text_color(muted).child("把光标放进要填写的单元格再打开本窗口；其它单元格按数据源填写。"))
                .child(div().text_sm().child("名单数据："))
                .child(Textarea::new(&names))
                .child(h_flex().justify_end().child(Button::new("bi-pages").primary().label("批量建页").on_click(move |_: &ClickEvent, w, cx| {
                    let lines: Vec<String> = names.read(cx).value().lines().map(str::to_string).collect();
                    let chars = view.read(cx).doc_chars().to_vec();
                    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
                    match tt.batch_pages(&chars, &lines, target, seed) {
                        Ok(edits) => {
                            let n = lines.iter().filter(|l| !l.trim().is_empty()).count();
                            view.update(cx, |v, cx| v.replace_many(edits, cx));
                            w.push_notification(Notification::success(format!("已新建 {n} 页")), cx);
                            w.close_dialog(cx);
                        }
                        Err(e) => w.push_notification(Notification::warning(e), cx),
                    }
                })))
                .into_any_element()
        };
        d.title("批量导入").w(px(480.)).child(
            v_flex()
                .gap_3()
                .child(
                    TabBar::new("bi-tab")
                        .small()
                        .segmented()
                        .selected_index(tab.get())
                        .children([Tab::new().label("批量导入数据"), Tab::new().label("批量名单建页")])
                        .on_click(move |ix: &usize, _w, cx| {
                            set.set(*ix);
                            cx.refresh_windows();
                        }),
                )
                .child(body),
        )
    });
}

/// Pick a workbook and fill the form from it (one undo step).
fn import_file(view: Entity<DocView>, window: &mut Window, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("选择表格文件（xlsx / xls / ods）".into()) });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = cx.update(|w, cx| {
                match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| chixie_rs_template::xlsx::read_sheets(&b)) {
                    Ok(sheets) => import_workbook(view, &sheets, w, cx),
                    Err(e) => w.push_notification(Notification::error(format!("导入失败：{e}")), cx),
                }
            });
        })
        .detach();
}

/// Fill the form's cells from a workbook's sheets (one undo step).
pub fn import_workbook(view: Entity<DocView>, sheets: &[chixie_rs_template::xlsx::Sheet], w: &mut Window, cx: &mut App) {
    let Some(TemplateKind::Table(tt)) = cx.global::<AppState>().active_template().map(|t| t.kind) else { return };
    let chars = view.read(cx).doc_chars().to_vec();
    match tt.import_sheets(&chars, sheets) {
        Ok(edits) => {
            view.update(cx, |v, cx| v.replace_many(edits, cx));
            w.push_notification(Notification::success(format!("已导入 {} 个工作表", sheets.len())), cx);
        }
        Err(e) => w.push_notification(Notification::warning(e), cx),
    }
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
