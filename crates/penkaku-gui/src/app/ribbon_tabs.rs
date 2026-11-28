//! Ribbon tab bodies. One `#[inline(never)]` fn per tab: element builders are large values, and
//! one function holding every tab's temporaries overflowed the 1 MB debug main-thread stack.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::select::Select;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, ThemeMode, h_flex, v_flex};
use penkaku_core::handwriting::StyleFile;
use penkaku_core::layout::{Align, Ruling};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{PenKakuApp, PageTool, Stage, fmt_num};
use penkaku_device::job::PatrolMode;
use crate::doc_view::{self, DocView};
use crate::ribbon::{self, big, big_menu, group, small, stack};
use crate::state::AppState;
use crate::templates_view::{kind_icon, preview};

const PAPERS: [(&str, (f32, f32)); 4] = [("A4", (210.0, 297.0)), ("A5", (148.0, 210.0)), ("B5", (176.0, 250.0)), ("16K", (184.0, 260.0))];
/// Word's margin presets (mm) as (left, top, right, bottom).
type Margins = (f32, f32, f32, f32);
const MARGINS: [(&str, Margins); 4] = [
    ("普通", (21.0, 20.0, 21.0, 20.0)),
    ("窄", (12.7, 12.7, 12.7, 12.7)),
    ("适中", (19.1, 25.4, 19.1, 25.4)),
    ("宽", (31.8, 25.4, 31.8, 25.4)),
];
/// Widths of the multi-row groups (each row fills its group); each keeps ~10 px of slack so
/// a control never gets clipped.
const FONT_GROUP_W: f32 = 232.0;
const PARA_GROUP_W: f32 = 164.0;
const SCRIPT_GROUP_W: f32 = 264.0;
const STRENGTH_GROUP_W: f32 = 300.0;

/// Click handler that runs an editor action with focus handed back to the page (so ribbon
/// buttons behave exactly like their shortcuts).
pub fn editor_action(doc: Entity<DocView>, a: Box<dyn Action>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        let focus = doc.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        window.dispatch_action(a.boxed_clone(), cx);
    }
}

/// 一键按手写整理 (`all`) or 箭头写成一笔. When the text would get an arrow the writing fonts
/// lack (drawn from their `-` and `>`, or not at all), ask first: 转换 writes the arrow, 保持原样
/// keeps `->` as typed and still does the rest of 一键整理.
fn clean_with_arrows(doc: Entity<DocView>, all: bool, window: &mut Window, cx: &mut App) {
    use gpui_kit::component::notification::Notification;
    let run = move |doc: &Entity<DocView>, convert: bool, w: &mut Window, cx: &mut App| {
        let n = doc.update(cx, |d, cx| d.clean(|e| if all { e.clean_for_handwriting(convert) } else if convert { e.join_arrows() } else { 0 }, cx));
        let msg = match (n, all) {
            (0, true) => "已经符合手写习惯，没有改动".to_string(),
            (0, false) => "没有需要替换的箭头".to_string(),
            (n, true) => format!("已整理 {n} 处"),
            (n, false) => format!("已替换 {n} 处"),
        };
        w.push_notification(Notification::info(msg), cx);
    };
    let arrows = doc.read(cx).arrows_to_join();
    let (composed, missing) = cx.global::<AppState>().arrow_coverage(&arrows);
    if composed.is_empty() && missing.is_empty() {
        run(&doc, true, window, cx);
        return;
    }
    let quote = |v: &[char]| v.iter().map(|c| format!("「{c}」")).collect::<String>();
    let mut text = String::new();
    if !composed.is_empty() {
        text += &format!("当前字体里没有{}。转换后会用字体自己的「-」「>」拼成一笔写出，箭杆和箭头连写，看起来像手写，但不是字体原有的字形。", quote(&composed));
    }
    if !missing.is_empty() {
        text += &format!("当前字体里没有{}，也拼不出来（缺少「-」「=」或「<」「>」），转换后会显示为缺字。", quote(&missing));
    }
    text += if all { "\n\n选「保持原样」：箭头保留为 -> 这样的两个字符，其他整理照常进行。" } else { "\n\n选「保持原样」：不做改动。" };
    window.open_dialog(cx, move |d, _w, _cx| {
        let (yes, no) = (doc.clone(), doc.clone());
        d.title("箭头写成一笔？").w(px(440.)).child(div().text_sm().child(text.clone())).footer(
            h_flex()
                .justify_end()
                .gap_2()
                .child(Button::new("arrow-keep").small().label("保持原样").on_click(move |_: &ClickEvent, w, cx| {
                    w.close_dialog(cx);
                    run(&no, false, w, cx);
                }))
                .child(Button::new("arrow-convert").small().primary().label("转换").on_click(move |_: &ClickEvent, w, cx| {
                    w.close_dialog(cx);
                    run(&yes, true, w, cx);
                })),
        )
    });
}

impl PenKakuApp {
    pub(super) fn ribbon_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.tab {
            0 => self.tab_home(cx),
            1 => self.tab_layout(cx),
            2 => self.tab_style(cx),
            3 => self.tab_device(cx),
            _ => self.tab_view(cx),
        };
        h_flex().h_full().px_1().child(body).into_any_element()
    }

    #[inline(never)]
    fn tab_home(&self, cx: &mut Context<Self>) -> AnyElement {
        let act = |a: Box<dyn Action>| editor_action(self.doc.clone(), a);
        // The first id is the 字体库… entry.
        let has_fonts = self.font_ids.len() > 1;
        let font_size = cx.global::<AppState>().settings.page.font_size;
        let ruling = cx.global::<AppState>().active_template().map(|t| t.ruling());
        let grid = matches!(ruling, Some(Ruling::Grid { .. }));
        let free_lines = ruling.as_ref().is_none_or(|r| matches!(r, Ruling::Free));
        let table = matches!(ruling, Some(Ruling::Table { .. }));
        let overflowing = self.doc.read(cx).layout_doc().overflowing().count();
        let fs = self.doc.read(cx).format_state(cx);
        let sel = fs.selected;
        let doc = self.doc.clone();
        let (align, indented) = {
            let d = self.doc.read(cx).doc();
            (d.align(), d.indented())
        };
        h_flex()
            .h_full()
            .child(group(
                cx,
                "clip",
                "剪贴板",
                h_flex().gap_1().child(big("paste", "粘贴", IconName::ClipboardPaste).on_click(act(Box::new(doc_view::Paste)))).child(stack([
                    small("cut", "剪切", IconName::Scissors).on_click(act(Box::new(doc_view::Cut))).into_any_element(),
                    small("copy", "复制", IconName::Copy).on_click(act(Box::new(doc_view::Copy))).into_any_element(),
                    small("painter", "格式刷", IconName::Paintbrush)
                        .selected(fs.painter)
                        .tooltip("单击：刷一次；双击：一直刷到按 Esc")
                        .on_click({
                            let doc = doc.clone();
                            move |e: &ClickEvent, _w, cx| doc.update(cx, |d, cx| d.arm_painter(e.click_count() > 1, cx))
                        })
                        .into_any_element(),
                ])),
                None,
            ))
            .child(group(
                cx,
                "font",
                "字体",
                ribbon::rows(
                    FONT_GROUP_W,
                    [
                        // The picker's first entry opens 字体库 (see `FontItem::library`).
                        ribbon::row()
                            .child(
                                div().flex_1().child(
                                    Select::new(&self.font_select)
                                        .small()
                                        .accessibility_label("字体")
                                        .placeholder(if sel && fs.font.is_none() {
                                            "所选内容中字体不统一"
                                        } else if has_fonts {
                                            "选择字体"
                                        } else {
                                            "（无收藏字体）"
                                        })
                                        .menu_width(px(300.)),
                                ),
                            )
                            .into_any_element(),
                        ribbon::row()
                            .child(ribbon::field_label("字号", cx))
                            .child(div().flex_1().child(NumberInput::new(&self.font_size).small().accessibility_label("字号")))
                            .child(ribbon::icon_btn("grow", IconName::AArrowUp, "增大字号").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                                t.bump_size(10.0, font_size, w, cx)
                            })))
                            .child(ribbon::icon_btn("shrink", IconName::AArrowDown, "减小字号").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                                t.bump_size(-10.0, font_size, w, cx)
                            })))
                            .child(self.pen_colour_menu(fs.colour))
                            .into_any_element(),
                        self.format_buttons(&fs),
                    ],
                ),
                None,
            ))
            .child(group(
                cx,
                "para",
                "段落",
                ribbon::rows(
                    PARA_GROUP_W,
                    [
                        ribbon::row()
                            .justify_between()
                            .child(ribbon::icon_btn("al-left", IconName::TextAlignStart, "左对齐").selected(align == Align::Left).on_click(act(Box::new(doc_view::AlignLeft))))
                            .child(ribbon::icon_btn("al-center", IconName::TextAlignCenter, "居中").selected(align == Align::Center).on_click(act(Box::new(doc_view::AlignCenter))))
                            .child(ribbon::icon_btn("al-right", IconName::TextAlignEnd, "右对齐").selected(align == Align::Right).on_click(act(Box::new(doc_view::AlignRight))))
                            .child(
                                ribbon::icon_btn("al-justify", IconName::TextAlignJustify, "两端对齐")
                                    .selected(align == Align::Justify)
                                    .on_click(act(Box::new(doc_view::AlignJustify))),
                            )
                            .child(
                                ribbon::icon_btn("indent", IconName::ListIndentIncrease, "首行缩进 (Tab)")
                                    .selected(indented)
                                    .on_click(act(Box::new(doc_view::ToggleIndent))),
                            )
                            .into_any_element(),
                        // 行距 only spaces free lines; ruled / grid / table templates set their own.
                        ribbon::row()
                            .child(ribbon::field_label("行距", cx))
                            .map(|el| {
                                if free_lines {
                                    el.child(div().flex_1().child(NumberInput::new(&self.line_height).small().accessibility_label("行距").suffix("mm")))
                                } else {
                                    el.child(div().text_xs().text_color(cx.theme().muted_foreground).child("由模板决定"))
                                }
                            })
                            .into_any_element(),
                        ribbon::row()
                            .child(
                                small("tune", "字符微调", IconName::Move)
                                    .tooltip("在右侧面板微调选中字符的位置、大小和变形")
                                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.open_tune(cx))),
                            )
                            // Only the composition grid can combine characters: hidden elsewhere.
                            .when(grid, |el| {
                                el.child(
                                    small("combine", "合并内容", IconName::Combine)
                                        .tooltip("选中的几个字写在同一格内（再点一次取消）")
                                        .on_click(act(Box::new(doc_view::Combine))),
                                )
                            })
                            .into_any_element(),
                    ],
                ),
                None,
            ))
            .child(self.edit_group(cx))
            .child(crate::insert_ui::insert_group(self.doc.clone(), cx))
            .when_some(crate::insert_ui::object_tools(self.doc.clone(), cx), |el, g| el.child(g))
            .when(table, |el| {
                let view = self.doc.clone();
                let tint = cx.theme().primary.opacity(0.06);
                el.child(group(
                    cx,
                    "table",
                    "表格功能",
                    h_flex().gap_1().child(stack([
                        small("next-cell", "下一格 (Tab)", IconName::ArrowRight).on_click(act(Box::new(doc_view::ToggleIndent))).into_any_element(),
                        small("prev-cell", "上一格 (Shift+Tab)", IconName::ArrowLeft).on_click(act(Box::new(doc_view::PrevCell))).into_any_element(),
                    ])).child(stack([
                        small("overflow", if overflowing > 0 { format!("批量溢出 ({overflowing})") } else { "批量溢出".to_string() }, IconName::TriangleAlert)
                            .on_click({
                                let view = view.clone();
                                move |_: &ClickEvent, w, cx| crate::table_ui::open_batch_overflow(view.clone(), w, cx)
                            })
                            .into_any_element(),
                        small("batch", "批量导入…", IconName::Import)
                            .on_click({
                                let view = view.clone();
                                move |_: &ClickEvent, w, cx| crate::table_ui::open_batch_import(view.clone(), w, cx)
                            })
                            .into_any_element(),
                        small("refresh", "刷新数据", IconName::RefreshCw)
                            .dropdown_menu(move |m, _w, _cx| {
                                let (v1, v2) = (view.clone(), view.clone());
                                m.item(PopupMenuItem::new("刷新数据(本页)").on_click(move |_, w, cx| crate::table_ui::refresh_data(&v1, false, w, cx)))
                                    .item(PopupMenuItem::new("刷新数据(所有页)").on_click(move |_, w, cx| crate::table_ui::refresh_data(&v2, true, w, cx)))
                            })
                            .into_any_element(),
                    ])),
                    None,
                )
                // Contextual group: tinted, like Word's 表格工具.
                .bg(tint))
            })
            .child(self.output_group(cx))
            .into_any_element()
    }

    /// 编辑: 查找 / 替换 and a 文本清理 menu (全选 and the two space clean-ups). 撤销 / 重做 live
    /// in the title bar's quick access toolbar only.
    #[inline(never)]
    fn edit_group(&self, cx: &mut Context<Self>) -> AnyElement {
        let doc = self.doc.clone();
        let done = |n: usize| if n == 0 { "没有需要消除的空格".to_string() } else { format!("已消除 {n} 处空格") };
        group(
            cx,
            "edit",
            "编辑",
            stack([
                small("find", "查找", IconName::Search)
                    .tooltip(format!("在导航窗格中搜索 ({})", ribbon::shortcut("F")))
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_nav(crate::nav_pane::NavTab::Search, w, cx)))
                    .into_any_element(),
                small("replace", "替换", IconName::Replace)
                    .tooltip(if cfg!(target_os = "macos") { "查找并替换 (⌥⌘F)".to_string() } else { "查找并替换 (Ctrl+H)".to_string() })
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_find(true, w, cx)))
                    .into_any_element(),
                small("text-clean", "文本清理", IconName::RemoveFormatting)
                    .dropdown_caret(true)
                    .tooltip("按手写习惯整理文字：空格、标点、箭头（有选中时只处理选中部分）")
                    .dropdown_menu(move |m, _w, _cx| {
                        // Each item runs one clean-up on the document and reports the count.
                        let item = |label: &'static str, f: fn(&mut penkaku_doc::EditDoc) -> usize, msg: fn(usize) -> String| {
                            let d = doc.clone();
                            PopupMenuItem::new(label).on_click(move |_, w, cx| {
                                let n = d.update(cx, |d, cx| d.clean(f, cx));
                                w.push_notification(gpui_kit::component::notification::Notification::info(msg(n)), cx);
                            })
                        };
                        let replaced = |n: usize| if n == 0 { "没有需要替换的符号".to_string() } else { format!("已替换 {n} 处") };
                        let (d_all, d_arrow) = (doc.clone(), doc.clone());
                        let removed = |n: usize| if n == 0 { "没有需要清理的内容".to_string() } else { format!("已清理 {n} 处") };
                        m.item(PopupMenuItem::new("全选").on_click(editor_action(doc.clone(), Box::new(doc_view::SelectAll))))
                            .separator()
                            .item(item("清理 Markdown 标记（# ** > --- 链接、emoji）", |d| d.clean_markdown(), removed))
                            .item(item("去句末句号（解题 / 手写模式，删段末的 。）", |d| d.drop_final_periods(), removed))
                            .separator()
                            .item(PopupMenuItem::new("一键按手写整理（下面全部）").on_click(move |_, w, cx| clean_with_arrows(d_all.clone(), true, w, cx)))
                            .separator()
                            .item(PopupMenuItem::new("箭头写成一笔（-> 改成 →）").on_click(move |_, w, cx| clean_with_arrows(d_arrow.clone(), false, w, cx)))
                            .item(item("标点改半角（，；：！？（） 改成 ,;:!?()）", |d| d.halve_punct(), replaced))
                            .item(item("消除符号两侧空格（-> = + < > 等）", |d| d.remove_operator_spaces(), done))
                            .item(item("消除中英空格（中文与英文、数字之间）", |d| d.remove_cjk_latin_spaces(), done))
                            .item(item("消除标点空格（英文标点后的正常空格保留）", |d| d.remove_punct_spaces(), done))
                    })
                    .into_any_element(),
            ]),
            None,
        )
        .into_any_element()
    }

    /// 书写与输出: the one primary action of the tab (书写 the selected page), page pick,
    /// G-code export and the device pane, so the 开始 tab closes the edit → write loop.
    #[inline(never)]
    fn output_group(&self, cx: &mut Context<Self>) -> AnyElement {
        let dev = self.device.read(cx);
        let (connected, running) = (dev.is_connected(), dev.job.is_some());
        let (page, pages) = (self.write_page(cx), self.doc.read(cx).layout_doc().pages.len().max(1));
        let write = big("home-write", "书写", IconName::PencilLine)
            .disabled(running)
            .tooltip(if connected { "书写选中的页" } else { "先连接设备（点击打开设备面板）" })
            .on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                if connected {
                    t.start_job(w, cx);
                } else {
                    t.show_device_pane();
                    w.push_notification(gpui_kit::component::notification::Notification::warning("请先连接设备"), cx);
                    cx.notify();
                }
            }));
        let pick = self.page_pick("home-page-pick", page, pages, cx);
        group(
            cx,
            "output",
            "书写与输出",
            h_flex()
                .gap_1()
                .child(if connected && !running { write.primary() } else { write })
                .child(stack([
                    pick,
                    small("home-export", "导出 G-code…", IconName::FileDown)
                        .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.export(w, cx)))
                        .into_any_element(),
                    small("home-pane", if self.show_pane { "隐藏面板" } else { "设备面板" }, IconName::PanelRight)
                        .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                            if t.show_pane {
                                t.show_pane = false;
                            } else {
                                t.show_device_pane();
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                ])),
            None,
        )
        .into_any_element()
    }

    /// 第 X / N 页 ▾: which page 书写 writes.
    fn page_pick(&self, id: &'static str, page: usize, pages: usize, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        small(id, format!("第 {} / {pages} 页", page + 1), IconName::File)
            .dropdown_caret(true)
            .tooltip("选择要书写的页")
            .dropdown_menu(move |m, _w, _cx| {
                (0..pages).fold(m, |m, k| {
                    let this = this.clone();
                    m.item(PopupMenuItem::new(format!("第 {} 页", k + 1)).checked(k == page).on_click(move |_, _w, cx| {
                        let _ = this.update(cx, |t, cx| t.goto_page(k, cx));
                    }))
                })
            })
            .into_any_element()
    }

    /// 中英文 group: letters and digits get their own font (role font), size (% of the Chinese
    /// size) and 字形 (the 手写效果 dialog's 英文 / 数字 tab); 消除中英空格.
    #[inline(never)]
    fn script_group(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::fonts_ctl::Role;
        const PCTS: [f32; 9] = [70.0, 75.0, 80.0, 85.0, 90.0, 95.0, 100.0, 110.0, 120.0];
        let st = cx.global::<AppState>();
        let hw = &st.settings.style.handwriting;
        let scales = [hw.english.scale_pct, hw.number.scale_pct];
        let fonts = st.fonts.list();
        // The button shows the role's font, so the row reads 英文 [巴拉体 ▾] [100% ▾] [字形].
        let font_name = |role: Role| -> SharedString {
            role.get(&st.settings.fonts.roles)
                .and_then(|key| fonts.iter().find(|f| f.id == key.local_id()))
                .map_or_else(|| "同中文".into(), |f| f.name.clone().into())
        };
        let names = [font_name(Role::English), font_name(Role::Number)];
        let app = cx.entity();
        let row = |k: usize, label: &'static str, role: Role| {
            let (a1, a2, a3) = (app.clone(), app.clone(), app.clone());
            let font = Button::new(("script-font", k))
                .xsmall()
                .outline()
                .h(px(ribbon::ROW_H))
                .w(px(92.))
                .justify_between()
                .child(div().flex_1().min_w_0().truncate().child(names[k].clone()))
                .dropdown_caret(true)
                .tooltip(format!("{}：{}", role.label(), names[k]))
                .dropdown_menu(move |m, _w, cx| {
                let st = cx.global::<AppState>();
                let current = role.get(&st.settings.fonts.roles).map(|key| key.local_id());
                let a = a1.clone();
                let m = m.item(PopupMenuItem::new("同中文（主线字体）").checked(current.is_none()).on_click(move |_, w, cx| a.update(cx, |t, cx| t.clear_role(role, w, cx))));
                st.fonts.list().into_iter().fold(m, |m, f| {
                    let a = a1.clone();
                    m.item(PopupMenuItem::new(f.name).checked(current == Some(f.id)).on_click(move |_, w, cx| a.update(cx, |t, cx| t.set_role(f.id, role, w, cx))))
                })
            });
            let size = Button::new(("script-size", k))
                .xsmall()
                .outline()
                .h(px(ribbon::ROW_H))
                .w(px(64.))
                .justify_between()
                .label(format!("{}%", fmt_num(scales[k])))
                .dropdown_caret(true)
                .tooltip("大小（相对中文字号）")
                .dropdown_menu(move |m, _w, cx| {
                let cur = cx.global::<AppState>().settings.style.handwriting.clone();
                let cur = if k == 0 { cur.english.scale_pct } else { cur.number.scale_pct };
                PCTS.iter().fold(m, |m, &p| {
                    let a = a2.clone();
                    m.item(PopupMenuItem::new(format!("{}%", fmt_num(p))).checked(cur == p).on_click(move |_, _w, cx| {
                        a.update(cx, |t, cx| {
                            t.edit_settings(cx, |st| {
                                let h = &mut st.settings.style.handwriting;
                                if k == 0 { h.english.scale_pct = p } else { h.number.scale_pct = p }
                            })
                        })
                    }))
                })
            });
            let shape = small(("script-shape", k), "字形", IconName::Settings2)
                // Distinct names: two buttons both called 字形 are ambiguous to UI Automation.
                .accessibility_label(if k == 0 { "英文字形设置" } else { "数字字形设置" })
                .tooltip(if k == 0 { "英文字形：基线、上下浮动和变形" } else { "数字字形：基线、上下浮动和变形" })
                .on_click(move |_: &ClickEvent, w, cx| a3.update(cx, |t, cx| t.open_style_tab(k + 1, w, cx)));
            ribbon::row().child(ribbon::field_label(label, cx)).child(font).child(size).child(shape).into_any_element()
        };
        let mixed = cx.global::<AppState>().settings.fonts.mix.len();
        group(
            cx,
            "script",
            "多语种与混用",
            ribbon::rows(
                SCRIPT_GROUP_W,
                [
                    row(0, "英文", Role::English),
                    row(1, "数字", Role::Number),
                    ribbon::row()
                        .child(
                            small("font-mix", if mixed > 0 { format!("多字体混用（{mixed} 款）…") } else { "多字体混用…".to_string() }, IconName::Shuffle)
                                .tooltip("在字体库中把多款字体加入混用，书写时随机换用")
                                .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_fonts(w, cx))),
                        )
                        .into_any_element(),
                ],
            ),
            None,
        )
        .into_any_element()
    }

    /// 笔色: the pen colour of the selected text (多色绘制 writes each colour with its own pen).
    #[inline(never)]
    fn pen_colour_menu(&self, colour: Option<Option<[u8; 3]>>) -> AnyElement {
        const PENS: [(&str, Option<[u8; 3]>); 4] = [("黑色（默认）", None), ("红色", Some([255, 0, 0])), ("蓝色", Some([0, 0, 255])), ("绿色", Some([0, 128, 0]))];
        let doc = self.doc.clone();
        // A bar under the palette shows the selection's pen (striped grey when mixed).
        let bar = match colour {
            Some(Some([r, g, b])) => gpui_kit::rgb(u32::from_be_bytes([0, r, g, b])).into(),
            Some(None) => gpui_kit::black(),
            None => gpui_kit::hsla(0., 0., 0.6, 1.),
        };
        Button::new("pen-colour")
            .ghost()
            .xsmall()
            .accessibility_label("笔色")
            .tooltip("笔色（选中文字）")
            .child(
                v_flex()
                    .items_center()
                    .child(Icon::new(IconName::Palette).with_size(px(14.)))
                    .child(div().w(px(14.)).h(px(3.)).mt(px(1.)).rounded_sm().bg(bar)),
            )
            .dropdown_menu(move |m, _w, _cx| {
                PENS.iter().fold(m, |m, &(name, c)| {
                    let doc = doc.clone();
                    m.item(PopupMenuItem::new(name).on_click(move |_, w, cx| {
                        if doc.read(cx).doc().selection().is_empty() {
                            w.push_notification(gpui_kit::component::notification::Notification::warning("请先选中要换色的文字"), cx);
                            return;
                        }
                        doc.update(cx, |d, cx| d.set_colour(c, cx));
                    }))
                })
            })
            .into_any_element()
    }

    /// 倾斜 ▾, 下划线, 删除线, 上标, 下标, 标题, 清除格式 (on the selection).
    #[inline(never)]
    fn format_buttons(&self, fs: &doc_view::FormatState) -> AnyElement {
        let act = |a: Box<dyn Action>| editor_action(self.doc.clone(), a);
        let doc = self.doc.clone();
        let (right, left) = (fs.tilt_right, fs.tilt_left);
        ribbon::row()
            .justify_between()
            .child(ribbon::icon_btn("tilt", IconName::Italic, "倾斜").selected(right || left).dropdown_menu(move |m, _w, _cx| {
                m.item(PopupMenuItem::new("右倾斜 (Ctrl+I)").checked(right).on_click(editor_action(doc.clone(), Box::new(doc_view::TiltRight))))
                    .item(PopupMenuItem::new("左倾斜").checked(left).on_click(editor_action(doc.clone(), Box::new(doc_view::TiltLeft))))
            }))
            .child(ribbon::icon_btn("underline", IconName::Underline, "下划线 (Ctrl+U)").selected(fs.underline).on_click(act(Box::new(doc_view::ToggleUnderline))))
            .child(ribbon::icon_btn("strike", IconName::Strikethrough, "删除线").selected(fs.strike).on_click(act(Box::new(doc_view::ToggleStrike))))
            .child(ribbon::icon_btn("sup", IconName::Superscript, "上标 (Ctrl+Shift+=)").selected(fs.sup).on_click(act(Box::new(doc_view::ToggleSuper))))
            .child(ribbon::icon_btn("sub", IconName::Subscript, "下标 (Ctrl+=)").selected(fs.sub).on_click(act(Box::new(doc_view::ToggleSub))))
            .child(ribbon::icon_btn("title", IconName::Heading, "标题 (Ctrl+B)").selected(fs.title).on_click(act(Box::new(doc_view::ToggleTitle))))
            .child(ribbon::icon_btn("clear-format", IconName::RemoveFormatting, "清除格式").on_click(act(Box::new(doc_view::ClearFormat))))
            .into_any_element()
    }

    /// A+ / A−: the selection's size, else the document's.
    fn bump_size(&mut self, delta: f32, doc_size: f32, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.read(cx).doc().selection().is_empty() {
            self.set_font_size(doc_size + delta, window, cx);
        } else {
            self.doc.update(cx, |d, cx| d.bump_run_size(delta, cx));
        }
    }

    fn set_font_size(&mut self, v: f32, window: &mut Window, cx: &mut Context<Self>) {
        let v = v.clamp(5.0, 1000.0);
        self.font_size.update(cx, |s, cx| s.set_value(fmt_num(v), window, cx));
        self.edit_settings(cx, |st| st.settings.page.font_size = v);
    }

    #[inline(never)]
    fn tab_layout(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let st = cx.global::<AppState>();
        let page = st.settings.page.clone();
        let active = page.template.clone();
        let templates = st.templates.templates.clone();
        let portrait = page.paper.0 <= page.paper.1;
        // 行距 only spaces free lines; ruled, grid and table templates set their own row pitch.
        let free_lines = st.active_template().is_none_or(|t| matches!(t.ruling(), Ruling::Free));
        let theme = cx.theme();
        let (accent, border, hover, muted) = (theme.primary, theme.border, theme.secondary, theme.muted_foreground);

        let w1 = this.clone();
        let paper_menu = big_menu("paper", "纸张大小", IconName::File).dropdown_menu(move |m, _w, _cx| {
            PAPERS.iter().fold(m, |m, (name, (w, h))| {
                let this = w1.clone();
                let (w, h) = (*w, *h);
                m.item(PopupMenuItem::new(format!("{name}  {w:.0} × {h:.0} mm")).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |t, cx| t.edit_page(cx, |p| p.paper = if p.paper.0 <= p.paper.1 { (w, h) } else { (h, w) }));
                }))
            })
        });
        let w2 = this.clone();
        let orient_menu = big_menu("orient", "纸张方向", if portrait { IconName::RectangleVertical } else { IconName::RectangleHorizontal })
            .dropdown_menu(move |m, _w, _cx| {
                let (a, b) = (w2.clone(), w2.clone());
                m.item(PopupMenuItem::new("纵向").icon(IconName::RectangleVertical).checked(portrait).on_click(move |_, _w, cx| {
                    let _ = a.update(cx, |t, cx| t.edit_page(cx, |p| p.paper = (p.paper.0.min(p.paper.1), p.paper.0.max(p.paper.1))));
                }))
                .item(PopupMenuItem::new("横向").icon(IconName::RectangleHorizontal).checked(!portrait).on_click(move |_, _w, cx| {
                    let _ = b.update(cx, |t, cx| t.edit_page(cx, |p| p.paper = (p.paper.0.max(p.paper.1), p.paper.0.min(p.paper.1))));
                }))
            });
        let w3 = this.clone();
        let current_margins = page.margins;
        let margin_menu = big_menu("margins", "页边距", IconName::Ruler).dropdown_menu(move |m, _w, _cx| {
            let m = MARGINS.iter().fold(m, |m, (name, mg)| {
                let this = w3.clone();
                let mg = *mg;
                m.item(
                    PopupMenuItem::new(format!("{name}  上 {:.1} 下 {:.1} 左 {:.1} 右 {:.1}", mg.1, mg.3, mg.0, mg.2))
                        .checked(mg == current_margins)
                        .on_click(move |_, _w, cx| {
                            let _ = this.update(cx, |t, cx| t.edit_page(cx, |p| p.margins = mg));
                        }),
                )
            });
            let this = w3.clone();
            m.separator().item(PopupMenuItem::new("自定义页面（模板管理）…").on_click(move |_, w, cx| {
                let _ = this.update(cx, |t, cx| t.open_backstage(Stage::Templates, w, cx));
            }))
        });

        // 排版规则 ▾: global rules, set once, so they live in a menu (full wording kept there).
        let w4 = this.clone();
        let rules = [
            ("标点避头尾：标点不出现在行首", page.kinsoku, 0usize),
            ("作文格：两个字母占一格（数字总是一格两个）", page.grid_pair_letters, 1),
            ("作文格分词：词语不跨行", page.grid_keep_words, 2),
        ];
        let typeset_menu = big_menu("typeset-rules", "排版规则", IconName::TextWrap).tooltip("避头尾、作文格字母和分词规则").dropdown_menu(move |m, _w, _cx| {
            rules.iter().fold(m, |m, &(label, on, k)| {
                let this = w4.clone();
                m.item(PopupMenuItem::new(label).checked(on).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |t, cx| {
                        t.edit_settings(cx, |st| {
                            let p = &mut st.settings.page;
                            match k {
                                0 => p.kinsoku = !on,
                                1 => p.grid_pair_letters = !on,
                                _ => p.grid_keep_words = !on,
                            }
                        })
                    });
                }))
            })
        });
        // Template gallery: 空白 + every stored template, the active one outlined.
        let card = |id: usize, name: String, icon: IconName, t: Option<penkaku_template::Template>, selected: bool| {
            v_flex()
                .id(("gallery", id))
                .w(px(68.))
                .h(px(72.))
                .p_0p5()
                .items_center()
                .rounded_md()
                .border_2()
                .border_color(if selected { accent } else { gpui_kit::transparent_black() })
                .when(selected, |el| el.bg(accent.opacity(0.08)))
                .hover(|s| s.bg(hover))
                .cursor_pointer()
                .child(div().w(px(42.)).h(px(50.)).child(match t {
                    Some(t) => preview(Some(t), gpui_kit::white(), border).into_any_element(),
                    None => div().size_full().flex().items_center().justify_center().child(Icon::new(icon).with_size(px(22.))).into_any_element(),
                }))
                .child(div().w_full().text_center().text_xs().truncate().when(selected, |el| el.text_color(accent).font_weight(FontWeight::SEMIBOLD)).child(name))
        };
        let mut gallery = h_flex().id("gallery").gap_1().max_w(px(540.)).overflow_x_scroll().child(
            card(0, "空白页".into(), IconName::File, None, active.is_none())
                .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.apply_template(None, cx))),
        );
        for (i, t) in templates.into_iter().enumerate() {
            let name = t.name.clone();
            let selected = active.as_deref() == Some(name.as_str());
            let icon = kind_icon(&t.kind);
            gallery = gallery.child(
                card(i + 1, name.clone(), icon, Some(t), selected).on_click(cx.listener(move |a, _: &ClickEvent, _w, cx| a.apply_template(Some(name.clone()), cx))),
            );
        }

        // Template first: it decides paper, margins and ruling; free paper settings come next.
        h_flex()
            .h_full()
            .child(group(
                cx,
                "templates",
                "模板",
                h_flex().gap_1().child(gallery).child(
                    big("manage", "管理模板", IconName::LayoutGrid)
                        .tooltip("新建、导入和自定义模板（含页面尺寸）")
                        .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::Templates, w, cx))),
                ),
                None,
            ))
            .child(group(
                cx,
                "page",
                if free_lines && active.is_none() { "纸张与边距" } else { "纸张与边距（修改将转为空白页）" },
                h_flex().gap_0p5().child(paper_menu).child(orient_menu).child(margin_menu),
                None,
            ))
            .child(group(
                cx,
                "typeset",
                "行距与排版",
                h_flex().gap_1().child(ribbon::rows(
                    150.0,
                    [
                        ribbon::row()
                            .child(ribbon::field_label("行距", cx))
                            .child(div().flex_1().child(NumberInput::new(&self.line_height).small().accessibility_label("行距").suffix("mm").disabled(!free_lines)))
                            .into_any_element(),
                        ribbon::row()
                            .text_xs()
                            .text_color(muted)
                            .child(if free_lines { "空白页的行间距" } else { "模板自带行距" })
                            .into_any_element(),
                    ],
                ))
                .child(typeset_menu),
                None,
            ))
            .into_any_element()
    }

    /// 风格预设 ▾: one-click switch + strength combinations (spacing / float ranges are untouched;
    /// those stay in the ↘ dialog).
    fn preset_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        small("style-preset", "风格预设", IconName::Settings2)
            .dropdown_caret(true)
            .tooltip("一键套用手写风格（只改开关和强度）")
            .dropdown_menu(move |m, _w, _cx| {
                [("工整（关闭随机与变形）", 0usize), ("轻微自然", 1), ("随性手写", 2)].into_iter().fold(m, |m, (label, k)| {
                    let this = this.clone();
                    m.item(PopupMenuItem::new(label).on_click(move |_, _w, cx| {
                        let _ = this.update(cx, |t, cx| t.edit_settings(cx, |st| apply_preset(&mut st.settings.style, k)));
                    }))
                })
                .separator()
                .item({
                    let this = this.clone();
                    PopupMenuItem::new("变形恢复推荐参数").on_click(move |_, _w, cx| {
                        let _ = this.update(cx, |t, cx| {
                            t.edit_settings(cx, |st| {
                                let d = &mut st.settings.style.deformation;
                                *d = penkaku_core::deformation::Deformation { enabled: d.enabled, base: d.base, shape: d.shape, ..penkaku_core::deformation::Deformation::recommended() };
                            })
                        });
                    })
                })
            })
            .into_any_element()
    }

    /// 手写强度: the dialog's most-used strengths as direct dropdowns; min/max ranges stay behind ↘.
    #[inline(never)]
    fn strength_group(&self, cx: &mut Context<Self>) -> AnyElement {
        const LEVELS: [f32; 11] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let style = cx.global::<AppState>().settings.style.clone();
        let (d, hw) = (&style.deformation, &style.handwriting);
        let app = cx.entity().downgrade();
        let level = |id: &'static str, label: &'static str, tip: &'static str, cur: f32, fmt: fn(f32) -> String, opts: &'static [f32], set: fn(&mut StyleFile, f32)| {
            let app = app.clone();
            Button::new(id)
                .xsmall()
                .outline()
                .h(px(ribbon::ROW_H))
                .label(format!("{label} {}", fmt(cur)))
                .dropdown_caret(true)
                .tooltip(tip)
                .dropdown_menu(move |m, _w, _cx| {
                    opts.iter().fold(m, |m, &v| {
                        let app = app.clone();
                        m.item(PopupMenuItem::new(fmt(v)).checked(v == cur).on_click(move |_, _w, cx| {
                            let _ = app.update(cx, |t, cx| t.edit_settings(cx, |st| set(&mut st.settings.style, v)));
                        }))
                    })
                })
                .into_any_element()
        };
        let plain: fn(f32) -> String = fmt_num;
        let pct: fn(f32) -> String = |v| format!("{}%", fmt_num(v));
        let steps: fn(f32) -> String = |v| format!("±{}级", fmt_num(v));
        let this = cx.entity().downgrade();
        let launcher: ribbon::ClickFn = Box::new(move |_, w, cx| {
            let _ = this.update(cx, |t, cx| t.open_style_dialog(w, cx));
        });
        group(
            cx,
            "strength",
            "手写强度",
            ribbon::rows(
                STRENGTH_GROUP_W,
                [
                    ribbon::row()
                        .child(level("warp-deg", "扭曲", "字形扭曲幅度（0 = 不扭曲；需开启字形变形）", d.shape_degree, plain, &LEVELS, |s, v| {
                            // Pin the slant first, so changing the warp doesn't move it.
                            s.deformation.slant_degree.get_or_insert(s.deformation.shape_degree);
                            s.deformation.shape_degree = v;
                        }))
                        .child(level("slant-deg", "倾斜", "字形倾斜幅度（需开启字形变形）", d.slant(), plain, &LEVELS, |s, v| s.deformation.slant_degree = Some(v)))
                        .child(level("stroke-deg", "笔画", "笔画扰动幅度（需开启字形变形）", d.stroke_degree, plain, &[0.0, 0.5, 1.0, 1.5, 2.0], |s, v| s.deformation.stroke_degree = v))
                        .into_any_element(),
                    ribbon::row()
                        .child(level("cursive-gap", "连笔距离", "相距多近的笔画会连起来（相对字高）", hw.cursive.max_gap_pct, pct, &[20.0, 30.0, 40.0, 60.0, 80.0, 100.0], |s, v| {
                            s.handwriting.cursive.max_gap_pct = v
                        }))
                        .child(level("cursive-chance", "连笔概率", "满足距离时真正连笔的概率", hw.cursive.chance_pct, pct, &[20.0, 40.0, 60.0, 80.0, 100.0], |s, v| {
                            s.handwriting.cursive.chance_pct = v
                        }))
                        .into_any_element(),
                    ribbon::row()
                        .child(level("press-jitter", "随机轻重", "每个字的落笔轻重随机变化（随机排布开启时生效）", hw.press_jitter, steps, &[0.0, 0.5, 1.0, 1.5, 2.0], |s, v| {
                            s.handwriting.press_jitter = v
                        }))
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("字距与浮动范围 ↘"))
                        .into_any_element(),
                ],
            ),
            Some(launcher),
        )
        .into_any_element()
    }

    #[inline(never)]
    fn tab_style(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = &cx.global::<AppState>().settings.style;
        let (hw, df) = (st.handwriting.clone(), st.deformation.clone());
        let (jit_on, base_on, shape_on) = (hw.enabled, df.enabled && df.base, df.enabled && df.shape);
        let this = cx.entity().downgrade();
        let launcher: ribbon::ClickFn = Box::new(move |_, w, cx| {
            let _ = this.update(cx, |t, cx| t.open_style_dialog(w, cx));
        });
        let preset = self.preset_menu(cx);
        h_flex()
            .h_full()
            .child(group(
                cx,
                "effects",
                "总开关与预设",
                h_flex().gap_1().child(stack([
                    ribbon::toggle("jitter", "随机排布", IconName::Shuffle, jit_on, "随机排布：字距、行首位置和上下浮动")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.style.handwriting.enabled = !jit_on);
                        }))
                        .into_any_element(),
                    ribbon::toggle("base", "基础变形", IconName::Scaling, base_on, "基础变形：胖瘦、高矮和旋转")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| {
                                let d = &mut st.settings.style.deformation;
                                d.base = !base_on;
                                d.enabled = d.base || d.shape;
                            });
                        }))
                        .into_any_element(),
                    ribbon::toggle("shape", "字形变形", IconName::Spline, shape_on, "字形变形：倾斜和笔画扰动")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| {
                                let d = &mut st.settings.style.deformation;
                                d.shape = !shape_on;
                                d.enabled = d.base || d.shape;
                            });
                        }))
                        .into_any_element(),
                ]))
                .child(stack([
                    ribbon::toggle("cursive", "连笔", IconName::Spline, hw.cursive.enabled, "连笔：同一字内相近的笔画不抬笔，用短弧线连起来")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.style.handwriting.cursive.enabled ^= true);
                        }))
                        .into_any_element(),
                    ribbon::toggle("drift", "智能偏移", IconName::Move, hw.intelligent_vertical_offset, "智能偏移：相邻字的上下浮动同向变化，更像真实书写")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.style.handwriting.intelligent_vertical_offset ^= true);
                        }))
                        .into_any_element(),
                    preset,
                ])),
                Some(launcher),
            ))
            .child(self.strength_group(cx))
            .child(self.script_group(cx))
            .child(group(
                cx,
                "seed",
                "笔迹样本",
                h_flex()
                    .gap_1()
                    .child(big("reshuffle", "换一组", IconName::Shuffle).tooltip("换一组随机笔迹（刷新随机种子）").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.reshuffle(cx))))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("#{}", hw.seed % 100_000))),
                None,
            ))
            .into_any_element()
    }

    #[inline(never)]
    fn tab_device(&self, cx: &mut Context<Self>) -> AnyElement {
        let dev = self.device.read(cx);
        let (connected, summary) = (dev.is_connected(), dev.summary());
        let alarm = dev.status.as_ref().is_some_and(|s| crate::device::state_zh(&s.state) == "报警");
        let job = dev.job;
        let running = job.is_some();
        let paused = job.is_some_and(|j| j.paused);
        let (page, pages) = (self.write_page(cx), self.doc.read(cx).layout_doc().pages.len().max(1));
        let pick = self.page_pick("write-page-pick", page, pages, cx);
        let muted = cx.theme().muted_foreground;
        let write_btn = big("start", "书写", IconName::PencilLine)
            .disabled(!connected || running)
            .tooltip("书写选中的页；写完后换纸，再选下一页书写")
            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_job(w, cx)));
        h_flex()
            .h_full()
            .child(group(
                cx,
                "conn",
                "连接与状态",
                h_flex()
                    .gap_1()
                    .child(if connected {
                        big("disconnect", "断开", IconName::Unplug)
                            .disabled(running)
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.disconnect(cx))))
                    } else {
                        big("connect", "连接设备", IconName::Plug).primary().on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                            t.show_device_pane();
                            t.device.update(cx, |d, cx| d.connect(cx));
                        }))
                    })
                    .child(
                        v_flex()
                            .w(px(150.))
                            .gap_1()
                            .child(div().text_xs().text_color(muted).child(summary))
                            // Alarm: unlock right here instead of hunting for it in the pane.
                            .when(alarm, |el| {
                                el.child(
                                    Button::new("ribbon-unlock")
                                        .xsmall()
                                        .danger()
                                        .label("解锁 $X")
                                        .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, _cx| d.send(penkaku_device::device::Command::Unlock)))),
                                )
                            }),
                    ),
                None,
            ))
            .child(group(
                cx,
                "write",
                "书写作业",
                h_flex()
                    .gap_0p5()
                    // Idle: 书写 is the one primary action. Running: 暂停/继续 + 停止 take its place.
                    .when(!running, |el| el.child(if connected { write_btn.primary() } else { write_btn }))
                    .when(running, |el| {
                        el.child(
                            big(if paused { "resume" } else { "pause" }, if paused { "继续" } else { "暂停" }, if paused { IconName::Play } else { IconName::Pause })
                                .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.toggle_pause(cx)))),
                        )
                        .child(big("stop", "停止", IconName::Square).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.stop(cx)))))
                    })
                    .child(stack([
                        pick,
                        small("write-resume", "断点续写", IconName::Redo)
                            .disabled(running)
                            .tooltip("从选中的字符继续书写被中断的页面")
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.open_resume(cx)))
                            .into_any_element(),
                        small("write-sd", "上传 SD 卡", IconName::Upload)
                            .disabled(running)
                            .tooltip("上传此页到无线控制板的SD卡并开始书写（炽写 ESP32 固件）")
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_sd_upload(w, cx)))
                            .into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "page-tools",
                "对位与校准",
                h_flex()
                    .gap_0p5()
                    .child(
                        big("patrol", "巡边", IconName::Square)
                            .disabled(!connected || running)
                            .tooltip("抬笔沿书写区域走一圈，检查纸张位置")
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_tool(PageTool::Patrol(PatrolMode::Up, false), w, cx))),
                    )
                    .child(stack([
                        {
                            let this = cx.entity().downgrade();
                            small("patrol-more", "巡边方式", IconName::ChevronDown)
                                .disabled(!connected || running)
                                .dropdown_menu(move |m, _w, _cx| {
                                    let combos = [false, true].into_iter().flat_map(|t| PatrolMode::ALL.into_iter().map(move |m| (m, t)));
                                    combos.fold(m, |m, (mode, template)| {
                                        let this = this.clone();
                                        let range = if template { "模板框线区域" } else { "实际书写区域" };
                                        m.item(PopupMenuItem::new(format!("{} · {range}", mode.label())).on_click(move |_, w, cx| {
                                            let _ = this.update(cx, |t, cx| t.start_tool(PageTool::Patrol(mode, template), w, cx));
                                        }))
                                    })
                                })
                                .into_any_element()
                        },
                        small("pen-calib-ribbon", "落笔校准…", IconName::PencilLine)
                            .disabled(!connected || running)
                            .tooltip("画一排深浅不同的线，点选最合适的一条来设定落笔高度")
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| {
                                t.show_device_pane();
                                t.device.update(cx, |d, cx| d.open_pen_calibration(w, cx));
                            }))
                            .into_any_element(),
                        small("ruling", "绘制框架线", IconName::Grid3x3)
                            .disabled(!connected || running)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_tool(PageTool::Ruling, w, cx)))
                            .into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "output",
                "输出与固件",
                h_flex()
                    .gap_0p5()
                    .child(big("export-gcode", "导出 G-code", IconName::FileDown).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.export(w, cx))))
                    .child(stack([
                        small("export-pdf", "导出 PDF…", IconName::FileDown)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::Export, w, cx)))
                            .into_any_element(),
                        small("print", "打印…", IconName::Printer)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::Export, w, cx)))
                            .into_any_element(),
                        small("firmware", "固件参数 ($)…", IconName::Settings2)
                            .disabled(!connected || running)
                            .tooltip("查看和修改控制板的 Grbl $ 参数")
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| crate::firmware::open(t.device.clone(), w, cx)))
                            .into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "pane",
                "工作区",
                h_flex().child(big("pane", if self.show_pane { "隐藏面板" } else { "设备面板" }, IconName::PanelRight).on_click(cx.listener(
                    |t, _: &ClickEvent, _w, cx| {
                        if t.show_pane {
                            t.show_pane = false;
                        } else {
                            t.show_device_pane();
                        }
                        cx.notify();
                    },
                ))),
                None,
            ))
            .into_any_element()
    }

    #[inline(never)]
    fn tab_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().mode == ThemeMode::Dark;
        let ui = cx.global::<AppState>().settings.ui.clone();
        let show_guides = ui.show_guides;
        let double = ui.double_page;
        let (nav, pane) = (self.nav_open(), self.show_pane);
        h_flex()
            .h_full()
            // 放大 / 缩小 live in the status bar; this keeps the one-click view modes.
            .child(group(
                cx,
                "pages",
                "显示模式",
                h_flex()
                    .gap_1()
                    .child(big("single", "单页", IconName::File).selected(!double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = false)
                    })))
                    .child(big("double", "双页", IconName::Columns2).selected(double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = true)
                    })))
                    .child(stack([
                        small("zfit", "页宽", IconName::StretchHorizontal)
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.doc.update(cx, |d, cx| d.fit_width(cx))))
                            .into_any_element(),
                        small("z100", "100%", IconName::ZoomIn)
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.doc.update(cx, |d, cx| d.set_zoom(doc_view::ZOOM_100, cx))))
                            .into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "show",
                "显示",
                stack([
                    ribbon::toggle("guides", "格线", IconName::Grid3x3, show_guides, "在页面上显示模板的格线（不会写出）")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.ui.show_guides = !show_guides);
                        }))
                        .into_any_element(),
                    ribbon::toggle("line-numbers", "行号", IconName::ListOrdered, ui.line_numbers, "在每行左侧显示行号")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.ui.line_numbers = !st.settings.ui.line_numbers);
                        }))
                        .into_any_element(),
                    ribbon::toggle("page-numbers", "页码", IconName::FileDigit, ui.page_numbers, "在每页下方显示页码")
                        .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                            t.edit_settings(cx, |st| st.settings.ui.page_numbers = !st.settings.ui.page_numbers);
                        }))
                        .into_any_element(),
                ]),
                None,
            ))
            .child(group(
                cx,
                "look",
                "外观与面板",
                h_flex()
                    .gap_1()
                    .child(big("dark", "深色模式", IconName::Moon).selected(dark).tooltip("切换深色 / 浅色界面").on_click(move |_: &ClickEvent, window, cx| {
                        ribbon::set_theme(!dark, Some(window), cx);
                        let st = cx.global_mut::<AppState>();
                        st.settings.ui.dark = !dark;
                        st.save_settings();
                    }))
                    .child(stack([
                        ribbon::toggle("nav-pane", "导航窗格", IconName::PanelLeft, nav, "左侧导航窗格：搜索与缺字")
                            .on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                                if nav {
                                    t.close_nav(w, cx);
                                } else {
                                    t.open_nav(crate::nav_pane::NavTab::Search, w, cx);
                                }
                            }))
                            .into_any_element(),
                        ribbon::toggle("side-pane", "设备与微调面板", IconName::PanelRight, pane, "右侧面板：设备控制 / 字符微调")
                            .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                                t.show_pane = !pane;
                                cx.notify();
                            }))
                            .into_any_element(),
                    ])),
                None,
            ))
            .into_any_element()
    }
}

/// 风格预设: 0 工整, 1 轻微自然, 2 随性手写. Only switches and strengths change.
fn apply_preset(s: &mut StyleFile, k: usize) {
    let (hw, d) = (&mut s.handwriting, &mut s.deformation);
    match k {
        0 => {
            hw.enabled = false;
            hw.cursive.enabled = false;
            (d.enabled, d.base, d.shape) = (false, false, false);
        }
        1 => {
            hw.enabled = true;
            hw.cursive.enabled = false;
            (d.enabled, d.base, d.shape) = (true, true, true);
            (d.shape_degree, d.slant_degree, d.stroke_degree) = (3.0, Some(3.0), 0.5);
        }
        _ => {
            hw.enabled = true;
            hw.cursive.enabled = true;
            (d.enabled, d.base, d.shape) = (true, true, true);
            (d.shape_degree, d.slant_degree, d.stroke_degree) = (7.0, Some(7.0), 1.5);
        }
    }
}
