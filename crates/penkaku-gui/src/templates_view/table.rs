//! 表格模板 editor: the pages, tables and cells of a table form on an interactive preview. Click
//! a cell to select it (Shift / Ctrl add, drag selects a block); drag a table's inner lines to
//! resize columns and rows. Edits stay in the template draft until 保存.

use std::cell::Cell;
use std::rc::Rc;

use penkaku_core::layout::{Align, Overflow, VAlign};
use penkaku_template::TemplateKind;
use penkaku_template::table::{CellRange, CellRef, ColWidth, DataSource, SameType, Span, TableError, TableTemplate};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, NumberInput, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::TemplatesView;
use crate::ribbon;

/// Paper top-left on screen and px per mm.
type PaperGeom = (Point<Pixels>, f32);

/// Editor state of the table form being edited.
#[derive(Default)]
pub(super) struct TableEdit {
    pub page: usize,
    /// Selected (table, cell).
    pub sel: Vec<(usize, usize)>,
    /// Paper top-left on screen and px per mm, from the last paint.
    geom: Rc<Cell<Option<PaperGeom>>>,
    drag: Option<Drag>,
    /// 为新表格记忆此尺寸: rows, cols, fixed width.
    remembered: Option<(u32, u32, Option<f32>)>,
    /// 预览原始背景 off hides the page background.
    pub hide_bg: bool,
    /// The page background as loaded (file, opacity step) → its grey pixels and gpui image.
    pub bg_cache: std::cell::RefCell<Option<super::import::BgImage>>,
}

#[derive(Clone, Copy)]
enum Drag {
    Select { table: usize, from: Span },
    Col { table: usize, col: usize },
    Row { table: usize, row: usize },
}

/// How near (mm) a click must be to a table line to drag it.
const GRAB_MM: f32 = 1.2;
/// Where 插入表格 puts a new table's left edge (mm).
const TABLE_LEFT: f32 = 10.0;

fn num(e: &Entity<InputState>, cx: &App) -> Option<f32> {
    e.read(cx).value().trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn text(e: &Entity<InputState>, cx: &App) -> String {
    e.read(cx).value().to_string()
}

fn input(window: &mut Window, cx: &mut App, v: impl Into<SharedString>) -> Entity<InputState> {
    let v = v.into();
    cx.new(|cx| InputState::new(window, cx).default_value(v))
}

fn fmt(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn row(label: &'static str, el: impl IntoElement, muted: Hsla) -> Div {
    h_flex().gap_2().child(div().w(px(84.)).text_sm().text_color(muted).child(label)).child(div().flex_1().child(el))
}

impl TemplatesView {
    fn tt(&self) -> Option<&TableTemplate> {
        match &self.draft.as_ref()?.kind {
            TemplateKind::Table(tt) => Some(tt),
            _ => None,
        }
    }

    pub(super) fn edit_tt(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TableTemplate)) {
        self.edit_kind(cx, |k| {
            if let TemplateKind::Table(tt) = k {
                f(tt);
            }
        });
    }

    /// An edit ChiXie3 may refuse: on error nothing changes and its message is shown.
    fn try_tt(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut TableTemplate) -> Result<(), TableError>) {
        let Some(mut tt) = self.tt().cloned() else { return };
        match f(&mut tt) {
            Ok(()) => self.edit_tt(cx, |t| *t = tt),
            Err(e) => window.push_notification(Notification::warning(e.to_string()), cx),
        }
    }

    pub(super) fn reset_table_edit(&mut self) {
        let geom = self.tbl.geom.clone();
        let remembered = self.tbl.remembered;
        self.tbl = TableEdit { geom, remembered, ..Default::default() };
    }

    fn to_mm(&self, p: Point<Pixels>) -> Option<(f32, f32)> {
        let (o, z) = self.tbl.geom.get()?;
        Some((f32::from(p.x - o.x) / z, f32::from(p.y - o.y) / z))
    }

    /// The cell under (x, y) mm on the current page.
    fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let p = self.tt()?.pages.get(self.tbl.page)?;
        p.tables.iter().enumerate().find_map(|(ti, t)| {
            t.cells.iter().position(|c| {
                let (cx0, cy0, w, h) = t.rect(c.span);
                x >= cx0 && x < cx0 + w && y >= cy0 && y < cy0 + h
            })
            .map(|ci| (ti, ci))
        })
    }

    fn tbl_down(&mut self, e: &MouseDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let Some((x, y)) = self.to_mm(e.position) else { return };
        let Some(p) = self.tt().and_then(|tt| tt.pages.get(self.tbl.page)) else { return };
        // A table's inner or right / bottom line: drag it.
        for (ti, t) in p.tables.iter().enumerate().filter(|(_, t)| !t.locked) {
            if y >= t.y && y <= t.y + t.height() {
                let mut edge = t.x;
                for (c, w) in t.cols.iter().enumerate() {
                    edge += w;
                    if (x - edge).abs() < GRAB_MM {
                        self.tbl.drag = Some(Drag::Col { table: ti, col: c });
                        return;
                    }
                }
            }
            if x >= t.x && x <= t.x + t.width() {
                let mut edge = t.y;
                for (r, h) in t.rows.iter().enumerate() {
                    edge += h;
                    if (y - edge).abs() < GRAB_MM {
                        self.tbl.drag = Some(Drag::Row { table: ti, row: r });
                        return;
                    }
                }
            }
        }
        let add = e.modifiers.shift || e.modifiers.control || e.modifiers.platform;
        match self.cell_at(x, y) {
            Some(hit) => {
                if add {
                    if let Some(k) = self.tbl.sel.iter().position(|s| *s == hit) {
                        self.tbl.sel.remove(k);
                    } else {
                        self.tbl.sel.push(hit);
                    }
                } else {
                    self.tbl.sel = vec![hit];
                }
                let span = self.tt().map(|tt| tt.pages[self.tbl.page].tables[hit.0].cells[hit.1].span);
                self.tbl.drag = span.map(|from| Drag::Select { table: hit.0, from });
            }
            None if !add => self.tbl.sel.clear(),
            None => {}
        }
        cx.notify();
    }

    fn tbl_move(&mut self, e: &MouseMoveEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let (Some(drag), Some((x, y))) = (self.tbl.drag, self.to_mm(e.position)) else { return };
        let page = self.tbl.page;
        match drag {
            Drag::Col { table, col } => self.edit_tt(cx, |tt| {
                let t = &mut tt.pages[page].tables[table];
                let left = t.x + t.cols[..col].iter().sum::<f32>();
                t.set_col_width(col, ((x - left) * 2.0).round() / 2.0);
            }),
            Drag::Row { table, row } => self.edit_tt(cx, |tt| {
                let t = &mut tt.pages[page].tables[table];
                let top = t.y + t.rows[..row].iter().sum::<f32>();
                t.set_row_height(row, ((y - top) * 2.0).round() / 2.0);
            }),
            Drag::Select { table, from } => {
                let Some((ti, ci)) = self.cell_at(x, y).filter(|h| h.0 == table) else { return };
                let Some(t) = self.tt().map(|tt| &tt.pages[page].tables[ti]) else { return };
                let to = t.cells[ci].span;
                let bb = Span { r0: from.r0.min(to.r0), c0: from.c0.min(to.c0), r1: from.r1.max(to.r1), c1: from.c1.max(to.c1) };
                let sel: Vec<(usize, usize)> = (0..t.cells.len())
                    .filter(|&k| {
                        let s = t.cells[k].span;
                        s.r0 < bb.r1 && bb.r0 < s.r1 && s.c0 < bb.c1 && bb.c0 < s.c1
                    })
                    .map(|k| (ti, k))
                    .collect();
                if sel != self.tbl.sel {
                    self.tbl.sel = sel;
                    cx.notify();
                }
            }
        }
    }

    fn tbl_up(&mut self, _: &MouseUpEvent, _w: &mut Window, _cx: &mut Context<Self>) {
        self.tbl.drag = None;
    }

    /// The paper with the current page's tables, cells, rows and selection.
    pub(super) fn table_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (accent, line, faint, mark) = (theme.primary, gpui_kit::black().opacity(0.75), theme.border, theme.warning);
        let t = self.draft.clone();
        let (page, sel, geom) = (self.tbl.page, self.tbl.sel.clone(), self.tbl.geom.clone());
        let bg = self.background(cx);
        div()
            .size_full()
            // A PDF or image dropped on the page is imported (导入 PDF/图片).
            .on_drop(cx.listener(|v, paths: &ExternalPaths, w, cx| {
                if let Some(p) = paths.paths().first() {
                    v.import_path(p.clone(), w, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::tbl_down))
            .on_mouse_move(cx.listener(Self::tbl_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::tbl_up))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _cx| {
                        let Some(t) = &t else { return };
                        let TemplateKind::Table(tt) = &t.kind else { return };
                        let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let z = (bw / t.paper.0).min(bh / t.paper.1) * 0.96;
                        let o = bounds.origin + point(px((bw - t.paper.0 * z) / 2.0), px((bh - t.paper.1 * z) / 2.0));
                        geom.set(Some((o, z)));
                        let paper = Bounds::new(o, size(px(t.paper.0 * z), px(t.paper.1 * z)));
                        window.paint_quad(fill(paper, gpui_kit::white()));
                        window.paint_quad(outline(paper, faint, BorderStyle::Solid));
                        let Some(p) = tt.pages.get(page) else { return };
                        let at = |x: f32, y: f32| o + point(px(x * z), px(y * z));
                        // The scanned form under the table (原始背景).
                        if let Some((b, image)) = &bg {
                            let r = Bounds::from_corners(at(b.x, b.y), at(b.x + b.w, b.y + b.h));
                            let _ = window.paint_image(paper, r, Corners::default(), image.clone(), 0, false);
                        }
                        for (ti, tb) in p.tables.iter().enumerate() {
                            for (ci, c) in tb.cells.iter().enumerate() {
                                let (x, y, w, h) = tb.rect(c.span);
                                let r = Bounds::from_corners(at(x, y), at(x + w, y + h));
                                if sel.contains(&(ti, ci)) {
                                    window.paint_quad(fill(r, accent.opacity(0.18)));
                                }
                                // 内部行数: light row lines inside the cell.
                                let (_, pt, _, pb) = c.padding;
                                let pitch = (h - pt - pb) / c.rows.max(1) as f32;
                                for k in 1..c.rows {
                                    let yy = y + pt + k as f32 * pitch;
                                    window.paint_quad(fill(Bounds::from_corners(at(x + 0.5, yy), at(x + w - 0.5, yy) + point(px(0.), px(1.))), faint));
                                }
                                window.paint_quad(outline(r, line, BorderStyle::Solid));
                                // A filled cell (data source) gets a small mark.
                                if c.data != DataSource::None {
                                    let s = px(1.6 * z);
                                    window.paint_quad(fill(Bounds::new(at(x + w, y) - point(s, px(0.)), size(s, s)), mark));
                                }
                            }
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    /// Pages, tables and cells of a table form (left panel).
    pub(super) fn table_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let Some(tt) = self.tt() else { return div().into_any_element() };
        let pages = tt.pages.len();
        let page = self.tbl.page.min(pages - 1);
        let has_sel = !self.tbl.sel.is_empty();
        let range = tt.pages[page].excel_range.map_or("未设置".to_string(), |r| r.to_string());
        let (empty, has_bg) = (tt.pages[page].tables.is_empty(), tt.pages[page].background.is_some());
        let section = |title: &'static str| div().pt_2().text_sm().font_weight(FontWeight::SEMIBOLD).child(title);
        let chips = (0..pages).map(|k| {
            Button::new(("tpage", k)).xsmall().label(format!("第{}页", k + 1)).when(k == page, |b| b.primary()).when(k != page, |b| b.outline()).on_click(cx.listener(move |v, _: &ClickEvent, _w, cx| {
                v.tbl.page = k;
                v.tbl.sel.clear();
                cx.notify();
            }))
        });
        v_flex()
            .gap_1p5()
            .child(section("页面"))
            .child(h_flex().gap_1().flex_wrap().children(chips))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(ribbon::small("tp-add", "增加空白页", IconName::FilePlus).on_click(cx.listener(|v, _: &ClickEvent, _w, cx| {
                        v.edit_tt(cx, |tt| tt.add_page());
                        v.tbl.page = v.tt().map_or(0, |tt| tt.pages.len() - 1);
                    })))
                    .child(ribbon::small("tp-dup", "复制", IconName::Copy).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.confirm_duplicate(w, cx))))
                    .child(ribbon::small("tp-up", "上移", IconName::ArrowUp).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.move_page(true, w, cx))))
                    .child(ribbon::small("tp-down", "下移", IconName::ArrowDown).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.move_page(false, w, cx))))
                    .child(ribbon::small("tp-del", "删除", IconName::Trash).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.confirm_delete_page(w, cx)))),
            )
            .child(section("表格"))
            .when(empty, |el| el.child(div().text_xs().text_color(muted).child("将文件拖到此处，或点击导入本地文件（PDF或图片表格）")))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(ribbon::small("t-import", "导入 PDF/图片…", IconName::Import).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.import_dialog(w, cx))))
                    .child(ribbon::small("t-ins", "插入表格…", IconName::Plus).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.insert_table_dialog(w, cx))))
                    .child(ribbon::small("t-pos", "表格位置…", IconName::Move).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.table_pos_dialog(w, cx))))
                    .child(ribbon::small("t-del", "删除表格", IconName::Trash).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, _w, cx| v.delete_table(cx)))),
            )
            .child(section("单元格"))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(ribbon::small("c-merge", "合并", IconName::Combine).disabled(self.tbl.sel.len() < 2).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.merge(w, cx))))
                    .child(ribbon::small("c-split", "拆分", IconName::Grid3x3).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, _w, cx| v.split(cx))))
                    .child(ribbon::small("c-attr", "单元格属性…", IconName::Settings).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.cell_attrs_dialog(w, cx))))
                    .child(ribbon::small("c-data", "数据源…", IconName::Shuffle).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.data_source_dialog(w, cx))))
                    .child(ribbon::small("c-same", "同型标记…", IconName::Copy).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.same_type_dialog(w, cx))))
                    .child(ribbon::small("c-pad", "内边距识别", IconName::Search).on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.detect_padding(w, cx))))
                    .child(ribbon::small("c-pad0", "重置内边距", IconName::Undo2).disabled(!has_sel).on_click(cx.listener(|v, _: &ClickEvent, _w, cx| v.reset_padding(cx)))),
            )
            .when(has_bg, |el| {
                el.child(section("背景")).child(
                    h_flex()
                        .gap_1()
                        .child(ribbon::small("bg-show", if self.tbl.hide_bg { "预览原始背景" } else { "隐藏原始背景" }, IconName::File).on_click(cx.listener(|v, _: &ClickEvent, _w, cx| {
                            v.tbl.hide_bg = !v.tbl.hide_bg;
                            cx.notify();
                        })))
                        .child(div().text_sm().text_color(muted).child("透明度"))
                        .child(ribbon::icon_btn("bg-less", IconName::Minus, "更透明").on_click(cx.listener(|v, _: &ClickEvent, _w, cx| v.bump_opacity(-0.1, cx))))
                        .child(ribbon::icon_btn("bg-more", IconName::Plus, "更不透明").on_click(cx.listener(|v, _: &ClickEvent, _w, cx| v.bump_opacity(0.1, cx)))),
                )
            })
            .child(div().text_xs().text_color(muted).child("点击选择单元格，Shift / Ctrl 多选，拖动选择一块；拖动表格线调整列宽和行高。"))
            .child(section("Excel导入配置"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child(format!("本页范围：{range}")))
                    .child(Button::new("x-range").xsmall().outline().label("设置…").on_click(cx.listener(|v, _: &ClickEvent, w, cx| v.excel_range_dialog(w, cx)))),
            )
            .into_any_element()
    }

    // ---- pages ----

    fn move_page(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        let k = self.tbl.page;
        let mut to = k;
        self.try_tt(window, cx, |tt| {
            to = tt.move_page(k, up)?;
            Ok(())
        });
        self.tbl.page = to;
    }

    fn confirm_delete_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tt().is_some_and(|tt| tt.pages.len() <= 1) {
            window.push_notification(Notification::warning(TableError::LastOne.to_string()), cx);
            return;
        }
        let (k, view) = (self.tbl.page, cx.entity());
        window.open_alert_dialog(cx, move |a, _w, _cx| {
            let view = view.clone();
            a.title(format!("确认要删除页面 {} 吗？", k + 1)).confirm().ok_text("删除").on_ok(move |_, w, cx| {
                view.update(cx, |v, cx| {
                    v.try_tt(w, cx, |tt| tt.remove_page(k));
                    v.tbl.page = k.saturating_sub(1);
                    v.tbl.sel.clear();
                });
                true
            })
        });
    }

    fn confirm_duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (k, view) = (self.tbl.page, cx.entity());
        window.open_alert_dialog(cx, move |a, _w, _cx| {
            let view = view.clone();
            a.title(format!("确认复制模板页面的第 {} 页吗？（默认会复制为最后一页）", k + 1)).confirm().ok_text("复制").on_ok(move |_, _w, cx| {
                view.update(cx, |v, cx| v.edit_tt(cx, |tt| tt.duplicate_page(k)));
                true
            })
        });
    }

    // ---- tables ----

    fn insert_table_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (r, c, fixed) = self.tbl.remembered.unwrap_or((3, 3, None));
        let rows = input(window, cx, r.to_string());
        let cols = input(window, cx, c.to_string());
        let width = input(window, cx, fmt(fixed.unwrap_or(20.0)));
        let auto = Rc::new(Cell::new(fixed.is_none()));
        let remember = Rc::new(Cell::new(false));
        let view = cx.entity();
        let insert = move |v: &mut TemplatesView, r: u32, c: u32, w: ColWidth, keep: bool, window: &mut Window, cx: &mut Context<TemplatesView>| {
            let (page, paper_w) = (v.tbl.page, v.draft.as_ref().map_or(210.0, |t| t.paper.0));
            v.try_tt(window, cx, |tt| tt.insert_table(page, r, c, w, paper_w, TABLE_LEFT));
            if keep {
                v.tbl.remembered = Some((r, c, match w {
                    ColWidth::Fixed(x) => Some(x),
                    ColWidth::Auto => None,
                }));
            }
        };
        let insert = Rc::new(insert);
        window.open_dialog(cx, move |d, _w, cx| {
            let theme = cx.theme();
            let (muted, accent, border) = (theme.muted_foreground, theme.primary, theme.border);
            let (rows, cols, width, auto, remember, view, insert) = (rows.clone(), cols.clone(), width.clone(), auto.clone(), remember.clone(), view.clone(), insert.clone());
            // Quick grid: click a square for that many rows × columns (自动列宽).
            let grid = v_flex().gap_0p5().children((0..10u32).map(|r| {
                h_flex().gap_0p5().children((0..8u32).map(|c| {
                    let (view, insert) = (view.clone(), insert.clone());
                    div()
                        .id(("tq", r * 8 + c))
                        .size(px(14.))
                        .border_1()
                        .border_color(border)
                        .hover(|s| s.bg(accent.opacity(0.4)))
                        .cursor_pointer()
                        .on_click(move |_, w, cx| {
                            view.update(cx, |v, cx| insert(v, r + 1, c + 1, ColWidth::Auto, false, w, cx));
                            w.close_dialog(cx);
                        })
                }))
            }));
            let (a1, a2, rem) = (auto.clone(), auto.clone(), remember.clone());
            d.title("插入表格")
                .w(px(420.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_xs().text_color(muted).child("点选方格直接插入，或设置下面的尺寸："))
                        .child(grid)
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("表格尺寸"))
                        .child(row("列数：", NumberInput::new(&cols).small(), muted))
                        .child(row("行数：", NumberInput::new(&rows).small(), muted))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("列宽选择"))
                        .child(
                            RadioGroup::horizontal("t-width").children(["固定列宽", "自动列宽"]).selected_index(Some(if a1.get() { 1 } else { 0 })).on_click(move |ix, _w, cx| {
                                a2.set(*ix == 1);
                                cx.refresh_windows();
                            }),
                        )
                        .when(!auto.get(), |el| el.child(row("列宽：", NumberInput::new(&width).small().suffix("mm"), muted)))
                        .child(Checkbox::new("t-remember").small().label("为新表格记忆此尺寸").checked(rem.get()).on_click(move |on: &bool, _w, cx| {
                            rem.set(*on);
                            cx.refresh_windows();
                        })),
                )
                .footer(h_flex().justify_end().gap_2().child(Button::new("t-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx))).child(
                    Button::new("t-ok").primary().label("插入").on_click(move |_: &ClickEvent, w, cx| {
                        let n = |e: &Entity<InputState>| num(e, cx).map(|v| v.clamp(1.0, 60.0) as u32).unwrap_or(1);
                        let (r, c) = (n(&rows), n(&cols));
                        let wdt = if auto.get() { ColWidth::Auto } else { ColWidth::Fixed(num(&width, cx).unwrap_or(20.0).max(2.0)) };
                        let keep = remember.get();
                        view.update(cx, |v, cx| insert(v, r, c, wdt, keep, w, cx));
                        w.close_dialog(cx);
                    }),
                ))
        });
    }

    fn table_pos_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(&(ti, _)), Some(tt)) = (self.tbl.sel.first(), self.tt()) else { return };
        let t = tt.pages[self.tbl.page].tables[ti].clone();
        let (w, h, x, y) = (input(window, cx, fmt(t.width())), input(window, cx, fmt(t.height())), input(window, cx, fmt(t.x)), input(window, cx, fmt(t.y)));
        let locked = Rc::new(Cell::new(t.locked));
        let (view, page) = (cx.entity(), self.tbl.page);
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let (w, h, x, y, locked, view) = (w.clone(), h.clone(), x.clone(), y.clone(), locked.clone(), view.clone());
            let lk = locked.clone();
            d.title("表格位置与尺寸(mm)")
                .w(px(360.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(row("表格宽度：", NumberInput::new(&w).small(), muted))
                        .child(row("表格高度：", NumberInput::new(&h).small(), muted))
                        .child(row("横向坐标：", NumberInput::new(&x).small(), muted))
                        .child(row("纵向坐标：", NumberInput::new(&y).small(), muted))
                        .child(Checkbox::new("t-lock").small().label("位置锁定").checked(lk.get()).on_click(move |on: &bool, _w, cx| {
                            lk.set(*on);
                            cx.refresh_windows();
                        })),
                )
                .footer(h_flex().justify_end().child(Button::new("tp-ok").primary().label("确定").on_click(move |_: &ClickEvent, win, cx| {
                    let (nw, nh, nx, ny) = (num(&w, cx), num(&h, cx), num(&x, cx), num(&y, cx));
                    let lock = locked.get();
                    view.update(cx, |v, cx| {
                        v.edit_tt(cx, |tt| {
                            let t = &mut tt.pages[page].tables[ti];
                            // Width / height scale the columns / rows in proportion.
                            if let Some(nw) = nw.filter(|v| *v >= 2.0) {
                                let k = nw / t.width();
                                t.cols.iter_mut().for_each(|c| *c *= k);
                            }
                            if let Some(nh) = nh.filter(|v| *v >= 2.0) {
                                let k = nh / t.height();
                                t.rows.iter_mut().for_each(|r| *r *= k);
                            }
                            t.x = nx.unwrap_or(t.x).max(0.0);
                            t.y = ny.unwrap_or(t.y).max(0.0);
                            t.locked = lock;
                        })
                    });
                    win.close_dialog(cx);
                })))
        });
    }

    fn delete_table(&mut self, cx: &mut Context<Self>) {
        let Some(&(ti, _)) = self.tbl.sel.first() else { return };
        let page = self.tbl.page;
        self.edit_tt(cx, |tt| {
            tt.pages[page].tables.remove(ti);
            for st in &mut tt.same_type {
                st.cells.retain(|c| !(c.page == page && c.table == ti));
                st.cells.iter_mut().filter(|c| c.page == page && c.table > ti).for_each(|c| c.table -= 1);
            }
            tt.same_type.retain(|s| s.cells.len() >= 2);
        });
        self.tbl.sel.clear();
    }

    // ---- cells ----

    fn merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let table = self.tbl.sel[0].0;
        if self.tbl.sel.iter().any(|s| s.0 != table) {
            window.push_notification(Notification::warning(TableError::NotRectangle.to_string()), cx);
            return;
        }
        let cells: Vec<usize> = self.tbl.sel.iter().map(|s| s.1).collect();
        let page = self.tbl.page;
        let mut merged = None;
        self.try_tt(window, cx, |tt| {
            merged = Some(tt.pages[page].tables[table].merge(&cells)?);
            Ok(())
        });
        if let Some(m) = merged {
            self.tbl.sel = vec![(table, m)];
        }
    }

    fn split(&mut self, cx: &mut Context<Self>) {
        let page = self.tbl.page;
        let mut sel = self.tbl.sel.clone();
        // Highest index first: splitting removes the cell and appends the pieces.
        sel.sort_by_key(|s| std::cmp::Reverse(s.1));
        self.edit_tt(cx, |tt| {
            for (t, c) in sel {
                tt.pages[page].tables[t].split(c);
            }
        });
        self.tbl.sel.clear();
    }

    /// 表格单元格属性 for the selected cells (shown with the first one's values).
    fn cell_attrs_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(&(t0, c0)), Some(tt)) = (self.tbl.sel.first(), self.tt()) else { return };
        let c = tt.pages[self.tbl.page].tables[t0].cells[c0].clone();
        let rows = input(window, cx, c.rows.to_string());
        let hint = input(window, cx, c.hint.clone());
        let pads = [c.padding.0, c.padding.1, c.padding.2, c.padding.3].map(|v| input(window, cx, fmt(v)));
        let size = input(window, cx, fmt(c.size * 100.0));
        let accept = Rc::new(Cell::new(c.accept_import));
        let align = Rc::new(Cell::new(match c.align {
            Align::Center => 1,
            Align::Right => 2,
            _ => 0,
        }));
        let valign = Rc::new(Cell::new(match c.valign {
            VAlign::Top => 0,
            VAlign::Middle => 1,
            VAlign::Bottom => 2,
        }));
        let overflow = Rc::new(Cell::new(match c.overflow {
            Overflow::Shrink { .. } => 0,
            Overflow::AddRows { .. } => 1,
            Overflow::Clip => 2,
        }));
        let (view, page, sel) = (cx.entity(), self.tbl.page, self.tbl.sel.clone());
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let (rows, hint, pads, size, view, sel) = (rows.clone(), hint.clone(), pads.clone(), size.clone(), view.clone(), sel.clone());
            let (accept, align, valign, overflow) = (accept.clone(), align.clone(), valign.clone(), overflow.clone());
            let radio = |id: &'static str, labels: [&'static str; 3], cell: Rc<Cell<usize>>| {
                let set = cell.clone();
                RadioGroup::horizontal(id).children(labels).selected_index(Some(cell.get())).on_click(move |ix, _w, cx| {
                    set.set(*ix);
                    cx.refresh_windows();
                })
            };
            let acc = accept.clone();
            d.title("表格单元格属性")
                .w(px(460.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(Checkbox::new("ca-import").small().label("接受导入").checked(acc.get()).on_click(move |on: &bool, _w, cx| {
                            acc.set(*on);
                            cx.refresh_windows();
                        }))
                        .child(row("内部行数", NumberInput::new(&rows).small(), muted))
                        .child(row("提示标识：", Input::new(&hint).small(), muted))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("排版内边距"))
                        .child(h_flex().gap_2().child(row("左内边距", NumberInput::new(&pads[0]).small().suffix("mm"), muted)).child(row("右内边距", NumberInput::new(&pads[2]).small().suffix("mm"), muted)))
                        .child(h_flex().gap_2().child(row("上内边距", NumberInput::new(&pads[1]).small().suffix("mm"), muted)).child(row("下内边距", NumberInput::new(&pads[3]).small().suffix("mm"), muted)))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("单元格字体属性"))
                        .child(row("默认字号", NumberInput::new(&size).small().suffix("%"), muted))
                        .child(row("水平对齐", radio("ca-align", ["左", "居中", "右"], align.clone()), muted))
                        .child(row("垂直对齐", radio("ca-valign", ["上", "居中", "下"], valign.clone()), muted))
                        .child(row("内容溢出", radio("ca-over", ["缩放字符", "增加行数", "忽略"], overflow.clone()), muted)),
                )
                .footer(h_flex().justify_end().child(Button::new("ca-ok").primary().label("确定").on_click(move |_: &ClickEvent, w, cx| {
                    let rows_v = num(&rows, cx).map(|v| v.clamp(1.0, 50.0) as u32);
                    let hint_v = text(&hint, cx);
                    let pad: Vec<Option<f32>> = pads.iter().map(|p| num(p, cx).map(|v| v.max(0.0))).collect();
                    let size_v = num(&size, cx).map(|v| (v / 100.0).clamp(0.1, 3.0));
                    let (acc, al, va, ov) = (accept.get(), align.get(), valign.get(), overflow.get());
                    let sel = sel.clone();
                    view.update(cx, |v, cx| {
                        v.edit_tt(cx, |tt| {
                            for &(t, c) in &sel {
                                let cell = &mut tt.pages[page].tables[t].cells[c];
                                if let Some(r) = rows_v {
                                    cell.rows = r;
                                }
                                cell.hint = hint_v.clone();
                                let p = &mut cell.padding;
                                (p.0, p.1, p.2, p.3) = (pad[0].unwrap_or(p.0), pad[1].unwrap_or(p.1), pad[2].unwrap_or(p.2), pad[3].unwrap_or(p.3));
                                cell.size = size_v.unwrap_or(cell.size);
                                cell.accept_import = acc;
                                cell.align = [Align::Left, Align::Center, Align::Right][al];
                                cell.valign = [VAlign::Top, VAlign::Middle, VAlign::Bottom][va];
                                cell.overflow = match ov {
                                    0 => Overflow::default(),
                                    1 => Overflow::AddRows { max: cell.rows.max(1) * 4 },
                                    _ => Overflow::Clip,
                                };
                            }
                        })
                    });
                    w.close_dialog(cx);
                })))
        });
    }

    /// 表格模板数据源 for one cell.
    fn data_source_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tbl.sel.len() != 1 {
            window.push_notification(Notification::warning("暂时数据源设置不支持多选单元格"), cx);
            return;
        }
        let Some(tt) = self.tt() else { return };
        let (t0, c0) = self.tbl.sel[0];
        let data = tt.pages[self.tbl.page].tables[t0].cells[c0].data.clone();
        let (mode, sub) = match &data {
            DataSource::None => (0, 0),
            DataSource::Fixed { .. } => (1, 0),
            DataSource::RandomNumber { .. } => (2, 0),
            DataSource::RandomChoice { .. } => (2, 1),
            DataSource::FromImport { .. } => (3, 0),
        };
        let (mode, sub) = (Rc::new(Cell::new(mode)), Rc::new(Cell::new(sub)));
        let fixed = input(window, cx, if let DataSource::Fixed { value } = &data { value.clone() } else { String::new() });
        let (mn, mx, dec) = if let DataSource::RandomNumber { min, max, decimals } = data { (min, max, decimals) } else { (0.0, 100.0, 0) };
        let (min_i, max_i, dec_i) = (input(window, cx, mn.to_string()), input(window, cx, mx.to_string()), input(window, cx, dec.to_string()));
        let choices_text = if let DataSource::RandomChoice { choices } = &data { choices.join("\n") } else { String::new() };
        let choices = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 8).default_value(choices_text));
        let (r0, c0i) = if let DataSource::FromImport { row, col } = data { (row, col) } else { (1, 1) };
        let (row_i, col_i) = (input(window, cx, r0.to_string()), input(window, cx, c0i.to_string()));
        let (view, page) = (cx.entity(), self.tbl.page);
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let (mode, sub, view) = (mode.clone(), sub.clone(), view.clone());
            let (fixed, min_i, max_i, dec_i, choices, row_i, col_i) = (fixed.clone(), min_i.clone(), max_i.clone(), dec_i.clone(), choices.clone(), row_i.clone(), col_i.clone());
            let (m_set, s_set) = (mode.clone(), sub.clone());
            let page_body: AnyElement = match mode.get() {
                0 => div().text_sm().text_color(muted).child("无数据源模式，数据需要导入或者输入").into_any_element(),
                1 => row("默认值：", Input::new(&fixed).small(), muted).into_any_element(),
                2 => v_flex()
                    .gap_2()
                    .child(RadioGroup::horizontal("ds-sub").children(["数值范围内随机", "给定内容内随机(使用换行分隔)"]).selected_index(Some(sub.get())).on_click(move |ix, _w, cx| {
                        s_set.set(*ix);
                        cx.refresh_windows();
                    }))
                    .child(if sub.get() == 0 {
                        v_flex()
                            .gap_2()
                            .child(h_flex().gap_2().child(div().text_sm().text_color(muted).child("取值范围：")).child(div().w(px(90.)).child(NumberInput::new(&min_i).small())).child("～").child(div().w(px(90.)).child(NumberInput::new(&max_i).small())))
                            .child(row("小数点后位数：", NumberInput::new(&dec_i).small(), muted))
                            .into_any_element()
                    } else {
                        Textarea::new(&choices).into_any_element()
                    })
                    .into_any_element(),
                _ => v_flex()
                    .gap_2()
                    .child(div().text_sm().text_color(muted).child("绑定单元格（仅限于导入文件使用）（Excel模式）"))
                    .child(row("行号：", NumberInput::new(&row_i).small(), muted))
                    .child(row("列号：", NumberInput::new(&col_i).small(), muted))
                    .into_any_element(),
            };
            d.title("表格模板数据源")
                .w(px(460.))
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            TabBar::new("ds-mode")
                                .small()
                                .segmented()
                                .selected_index(mode.get())
                                .children([Tab::new().label("无数据源"), Tab::new().label("默认值"), Tab::new().label("随机值"), Tab::new().label("指定单元格")])
                                .on_click(move |ix: &usize, _w, cx| {
                                    m_set.set(*ix);
                                    cx.refresh_windows();
                                }),
                        )
                        .child(page_body),
                )
                .footer(h_flex().justify_end().child(Button::new("ds-ok").primary().label("保存").on_click(move |_: &ClickEvent, w, cx| {
                    let ds = match (mode.get(), sub.get()) {
                        (0, _) => DataSource::None,
                        (1, _) => DataSource::Fixed { value: text(&fixed, cx) },
                        (2, 0) => DataSource::RandomNumber {
                            min: num(&min_i, cx).unwrap_or(0.0) as f64,
                            max: num(&max_i, cx).unwrap_or(0.0) as f64,
                            decimals: num(&dec_i, cx).map_or(0, |v| v.clamp(0.0, 6.0) as u32),
                        },
                        (2, _) => DataSource::RandomChoice { choices: choices.read(cx).value().lines().map(str::to_string).collect() },
                        _ => DataSource::FromImport { row: num(&row_i, cx).map_or(1, |v| v.max(1.0) as u32), col: num(&col_i, cx).map_or(1, |v| v.max(1.0) as u32) },
                    };
                    if let Err(e) = ds.check() {
                        w.push_notification(Notification::warning(e), cx);
                        return;
                    }
                    view.update(cx, |v, cx| v.edit_tt(cx, |tt| tt.pages[page].tables[t0].cells[c0].data = ds));
                    w.close_dialog(cx);
                })))
        });
    }

    /// 同型标记: records of cells that hold the same kind of content.
    fn same_type_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = input(window, cx, "");
        let (view, page, sel) = (cx.entity(), self.tbl.page, self.tbl.sel.clone());
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let records = view.read(cx).tt().map(|tt| tt.same_type.clone()).unwrap_or_default();
            let (name, view2, sel) = (name.clone(), view.clone(), sel.clone());
            let list = v_flex().gap_1().children(records.iter().enumerate().map(|(i, r)| {
                let view = view.clone();
                h_flex()
                    .gap_2()
                    .child(div().flex_1().text_sm().child(format!("{}（{} 个单元格）", r.name, r.cells.len())))
                    .child(Button::new(("st-del", i)).xsmall().ghost().label("删除").on_click(move |_: &ClickEvent, _w, cx| {
                        view.update(cx, |v, cx| v.edit_tt(cx, |tt| {
                            tt.same_type.remove(i);
                        }));
                        cx.refresh_windows();
                    }))
            }));
            d.title("同型标记")
                .w(px(420.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("已有同型"))
                        .child(if records.is_empty() { div().text_sm().text_color(muted).child("（无）").into_any_element() } else { list.into_any_element() })
                        .child(row("同型备注：", Input::new(&name).small(), muted))
                        .child(div().text_xs().text_color(muted).child(format!("把当前选中的 {} 个单元格（第{}页）记为一组。", sel.len(), page + 1))),
                )
                .footer(h_flex().justify_end().child(Button::new("st-add").primary().label("添加").on_click(move |_: &ClickEvent, w, cx| {
                    let label = text(&name, cx);
                    let cells: Vec<CellRef> = sel.iter().map(|&(table, cell)| CellRef { page, table, cell }).collect();
                    let taken = view2.read(cx).tt().is_some_and(|tt| tt.same_type.iter().flat_map(|s| &s.cells).any(|c| cells.contains(c)));
                    let msg = if cells.len() < 2 {
                        Some("同型标记至少需要选中两个单元格")
                    } else if taken {
                        Some("单元格已在其他同型标记中")
                    } else {
                        None
                    };
                    if let Some(m) = msg {
                        w.push_notification(Notification::warning(m), cx);
                        return;
                    }
                    let label = if label.trim().is_empty() { "同型".to_string() } else { label };
                    view2.update(cx, |v, cx| v.edit_tt(cx, |tt| tt.same_type.push(SameType { name: label, cells })));
                    cx.refresh_windows();
                })))
        });
    }

    fn excel_range_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tt) = self.tt() else { return };
        let cur = tt.pages[self.tbl.page].excel_range;
        let letters = |c: u32| penkaku_template::table::column_letters(c);
        let (c0, r0, c1, r1) = cur.map_or(Default::default(), |r| (letters(r.first.0), r.first.1.to_string(), letters(r.last.0), r.last.1.to_string()));
        let fields = [input(window, cx, c0), input(window, cx, r0), input(window, cx, c1), input(window, cx, r1)];
        let (view, page) = (cx.entity(), self.tbl.page);
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let (f, view) = (fields.clone(), view.clone());
            d.title("Excel导入配置")
                .w(px(380.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("左上角"))
                        .child(h_flex().gap_2().child(row("左上列", Input::new(&f[0]).small(), muted)).child(row("左上行", Input::new(&f[1]).small(), muted)))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("右下角"))
                        .child(h_flex().gap_2().child(row("右下列", Input::new(&f[2]).small(), muted)).child(row("右下行", Input::new(&f[3]).small(), muted))),
                )
                .footer(h_flex().justify_end().gap_2().child(Button::new("x-clear").label("清除").on_click({
                    let view = view.clone();
                    move |_: &ClickEvent, w, cx| {
                        view.update(cx, |v, cx| v.edit_tt(cx, |tt| tt.pages[page].excel_range = None));
                        w.close_dialog(cx);
                    }
                })).child(Button::new("x-ok").primary().label("暂存").on_click(move |_: &ClickEvent, w, cx| {
                    let v: Vec<String> = f.iter().map(|e| text(e, cx)).collect();
                    match CellRange::from_fields(&v[0], &v[1], &v[2], &v[3]) {
                        Ok(r) => {
                            view.update(cx, |v, cx| v.edit_tt(cx, |tt| tt.pages[page].excel_range = Some(r)));
                            w.push_notification(Notification::success("暂存成功"), cx);
                            w.close_dialog(cx);
                        }
                        Err(e) => w.push_notification(Notification::warning(e.to_string()), cx),
                    }
                })))
        });
    }
}
