//! Ribbon tab bodies. One `#[inline(never)]` fn per tab: element builders are large values, and
//! one function holding every tab's temporaries overflowed the 1 MB debug main-thread stack.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::select::Select;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, ThemeMode, h_flex, v_flex};
use chixie_rs_core::layout::{Align, Ruling};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{ChixieRsApp, JobScope, PageTool, Stage, fmt_num};
use chixie_rs_device::job::PatrolMode;
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

/// Click handler that runs an editor action with focus handed back to the page (so ribbon
/// buttons behave exactly like their shortcuts).
pub fn editor_action(doc: Entity<DocView>, a: Box<dyn Action>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        let focus = doc.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        window.dispatch_action(a.boxed_clone(), cx);
    }
}

impl ChixieRsApp {
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
        let has_fonts = !self.font_ids.is_empty();
        let font_size = cx.global::<AppState>().settings.page.font_size;
        let pen_menu = self.pen_colour_menu();
        let ruling = cx.global::<AppState>().active_template().map(|t| t.ruling());
        let grid = matches!(ruling, Some(Ruling::Grid { .. }));
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
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div().w(px(190.)).child(
                                    Select::new(&self.font_select)
                                        .small()
                                        .placeholder(if sel && fs.font.is_none() {
                                            "所选内容中字体不统一"
                                        } else if has_fonts {
                                            "选择字体"
                                        } else {
                                            "（无收藏字体）"
                                        })
                                        .menu_width(px(260.)),
                                ),
                            )
                            .child(small("fontlib", "字体库…", IconName::Type).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_fonts(w, cx)))),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().text_xs().child("字号"))
                            .child(div().w(px(96.)).child(NumberInput::new(&self.font_size).small()))
                            .child(ribbon::icon_btn("grow", IconName::AArrowUp, "增大字号").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                                t.bump_size(10.0, font_size, w, cx)
                            })))
                            .child(ribbon::icon_btn("shrink", IconName::AArrowDown, "减小字号").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| {
                                t.bump_size(-10.0, font_size, w, cx)
                            })))
                            .child(pen_menu),
                    )
                    .child(self.format_buttons(&fs)),
                None,
            ))
            .child(group(
                cx,
                "para",
                "段落",
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_0p5()
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
                            ),
                    )
                    .child(h_flex().gap_1().child(div().text_xs().child("行距")).child(div().w(px(104.)).child(NumberInput::new(&self.line_height).small().suffix("mm"))))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                small("combine", "合并内容", IconName::Combine)
                                    .disabled(!grid)
                                    .tooltip(if grid { "选中的几个字写在同一格内（再点一次取消）" } else { "仅作文格可用" })
                                    .on_click(act(Box::new(doc_view::Combine))),
                            )
                            .child(
                                small("tune", "字符微调", IconName::Move)
                                    .tooltip("微调选中字符的位置、大小和变形")
                                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.open_tune(cx))),
                            ),
                    ),
                None,
            ))
            .when(table, |el| {
                let view = self.doc.clone();
                el.child(group(
                    cx,
                    "table",
                    "表格功能",
                    stack([
                        small("next-cell", "下一格 (Tab)", IconName::ArrowRight).on_click(act(Box::new(doc_view::ToggleIndent))).into_any_element(),
                        small("prev-cell", "上一格 (Shift+Tab)", IconName::ArrowLeft).on_click(act(Box::new(doc_view::PrevCell))).into_any_element(),
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
                    ]),
                    None,
                ))
            })
            .child(group(
                cx,
                "edit",
                "编辑",
                stack([
                    small("selall", "全选", IconName::TextCursorInput).on_click(act(Box::new(doc_view::SelectAll))).into_any_element(),
                    small("undo", "撤销", IconName::Undo2).on_click(act(Box::new(doc_view::Undo))).into_any_element(),
                    small("redo", "重做", IconName::Redo2).on_click(act(Box::new(doc_view::Redo))).into_any_element(),
                ]),
                None,
            ))
            .into_any_element()
    }

    /// 笔色: the pen colour of the selected text (多色绘制 writes each colour with its own pen).
    #[inline(never)]
    fn pen_colour_menu(&self) -> AnyElement {
        const PENS: [(&str, Option<[u8; 3]>); 4] = [("黑色（默认）", None), ("红色", Some([255, 0, 0])), ("蓝色", Some([0, 0, 255])), ("绿色", Some([0, 128, 0]))];
        let doc = self.doc.clone();
        ribbon::icon_btn("pen-colour", IconName::Palette, "笔色（选中文字）")
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
        h_flex()
            .gap_0p5()
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

        // Template gallery: 空白 + every stored template, the active one outlined.
        let card = |id: usize, name: String, icon: IconName, t: Option<chixie_rs_template::Template>, selected: bool| {
            v_flex()
                .id(("gallery", id))
                .w(px(64.))
                .h(px(72.))
                .p_0p5()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(if selected { accent } else { gpui_kit::transparent_black() })
                .hover(|s| s.bg(hover))
                .cursor_pointer()
                .child(div().w(px(42.)).h(px(50.)).child(match t {
                    Some(t) => preview(Some(t), gpui_kit::white(), border).into_any_element(),
                    None => div().size_full().flex().items_center().justify_center().child(Icon::new(icon).with_size(px(22.))).into_any_element(),
                }))
                .child(div().w_full().text_center().text_xs().truncate().child(name))
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

        h_flex()
            .h_full()
            .child(group(cx, "page", "页面设置", h_flex().gap_0p5().child(paper_menu).child(orient_menu).child(margin_menu), None))
            .child(group(
                cx,
                "templates",
                "模板",
                h_flex().gap_1().child(gallery).child(
                    big("manage", "管理模板", IconName::LayoutGrid).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::Templates, w, cx))),
                ),
                None,
            ))
            .child(group(
                cx,
                "typeset",
                "排版",
                stack([
                    Checkbox::new("kinsoku")
                        .small()
                        .label("避头尾（标点不出现在行首）")
                        .checked(page.kinsoku)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.page.kinsoku = v);
                        }))
                        .into_any_element(),
                    Checkbox::new("pair-letters")
                        .small()
                        .label("作文格：两个字母占一格（数字总是）")
                        .checked(page.grid_pair_letters)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.page.grid_pair_letters = v);
                        }))
                        .into_any_element(),
                    Checkbox::new("keep-words")
                        .small()
                        .label("作文格分词（词语不跨行）")
                        .checked(page.grid_keep_words)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.page.grid_keep_words = v);
                        }))
                        .into_any_element(),
                ]),
                None,
            ))
            .child(div().px_2().text_xs().text_color(muted).child(format!("{:.0} × {:.0} mm", page.paper.0, page.paper.1)))
            .into_any_element()
    }

    #[inline(never)]
    fn tab_style(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = &cx.global::<AppState>().settings.style;
        let (hw, df) = (st.handwriting.clone(), st.deformation.clone());
        let this = cx.entity().downgrade();
        let launcher: ribbon::ClickFn = Box::new(move |_, w, cx| {
            let _ = this.update(cx, |t, cx| t.open_style_dialog(w, cx));
        });
        h_flex()
            .h_full()
            .child(group(
                cx,
                "effects",
                "效果",
                stack([
                    Checkbox::new("jitter")
                        .small()
                        .label("随机排布（字距 / 行首 / 上下浮动）")
                        .checked(hw.enabled)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.style.handwriting.enabled = v);
                        }))
                        .into_any_element(),
                    Checkbox::new("base")
                        .small()
                        .label("基础变形（胖瘦 / 高矮 / 旋转）")
                        .checked(df.enabled && df.base)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| {
                                let d = &mut st.settings.style.deformation;
                                d.base = v;
                                d.enabled = d.base || d.shape;
                            });
                        }))
                        .into_any_element(),
                    Checkbox::new("shape")
                        .small()
                        .label("字形变形（倾斜 / 笔画扰动）")
                        .checked(df.enabled && df.shape)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| {
                                let d = &mut st.settings.style.deformation;
                                d.shape = v;
                                d.enabled = d.base || d.shape;
                            });
                        }))
                        .into_any_element(),
                ]),
                Some(launcher),
            ))
            .child(group(
                cx,
                "seed",
                "笔迹",
                h_flex()
                    .gap_1()
                    .child(big("reshuffle", "换一组", IconName::Shuffle).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.reshuffle(cx))))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("种子\n{}", hw.seed % 100_000))),
                None,
            ))
            .child(group(
                cx,
                "import",
                "导入",
                h_flex().child(big("import-style", "从炽写导入", IconName::Import).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.import_style(w, cx)))),
                None,
            ))
            .into_any_element()
    }

    #[inline(never)]
    fn tab_device(&self, cx: &mut Context<Self>) -> AnyElement {
        let dev = self.device.read(cx);
        let (connected, summary) = (dev.is_connected(), dev.summary());
        let job = dev.job;
        let running = job.is_some();
        let paused = job.is_some_and(|j| j.paused);
        let awaiting = job.and_then(|j| j.awaiting);
        h_flex()
            .h_full()
            .child(group(
                cx,
                "conn",
                "连接",
                h_flex()
                    .gap_1()
                    .child(if connected {
                        big("disconnect", "断开", IconName::Unplug)
                            .disabled(running)
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.disconnect(cx))))
                    } else {
                        big("connect", "连接设备", IconName::Plug).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                            t.show_pane = true;
                            t.device.update(cx, |d, cx| d.connect(cx));
                        }))
                    })
                    .child(div().w(px(150.)).text_xs().text_color(cx.theme().muted_foreground).child(summary)),
                None,
            ))
            .child(group(
                cx,
                "write",
                "书写",
                h_flex()
                    .gap_0p5()
                    .child(big("start", "开始书写", IconName::PencilLine).disabled(!connected || running).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_job(JobScope::All, w, cx))))
                    .child(stack([
                        small("write-page", "书写此页", IconName::File)
                            .disabled(!connected || running)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_job(JobScope::ThisPage, w, cx)))
                            .into_any_element(),
                        small("write-from", "从此页起", IconName::ArrowDown)
                            .disabled(!connected || running)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_job(JobScope::FromHere, w, cx)))
                            .into_any_element(),
                        small("write-resume", "断点续写", IconName::Redo)
                            .disabled(running)
                            .tooltip("从选中的字符继续书写被中断的页面")
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.open_resume(cx)))
                            .into_any_element(),
                    ]))
                    .child(stack([small("write-sd", "上传SD卡", IconName::Upload)
                        .disabled(running)
                        .tooltip("上传此页到无线控制板的SD卡并开始书写（炽写 ESP32 固件）")
                        .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_sd_upload(w, cx)))
                        .into_any_element()]))
                    .child(
                        big(if paused { "resume" } else { "pause" }, if paused { "继续" } else { "暂停" }, if paused { IconName::Play } else { IconName::Pause })
                            .disabled(!running || awaiting.is_some())
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.toggle_pause(cx)))),
                    )
                    .child(big("stop", "停止", IconName::Square).disabled(!running).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.update(cx, |d, cx| d.stop(cx)))))
                    .when(awaiting.is_some(), |el| {
                        el.child(Button::new("next").small().primary().label("已换纸，继续").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                            t.device.update(cx, |d, cx| d.next_sheet(cx))
                        })))
                    }),
                None,
            ))
            .child(group(
                cx,
                "page-tools",
                "巡边",
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
                        small("ruling", "绘制框架线", IconName::Grid3x3)
                            .disabled(!connected || running)
                            .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_tool(PageTool::Ruling, w, cx)))
                            .into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "pane",
                "窗格",
                h_flex().child(big("pane", if self.show_pane { "隐藏面板" } else { "设备面板" }, IconName::PanelRight).on_click(cx.listener(
                    |t, _: &ClickEvent, _w, cx| {
                        t.show_pane = !t.show_pane;
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
        h_flex()
            .h_full()
            .child(group(
                cx,
                "pages",
                "页面",
                h_flex()
                    .gap_1()
                    .child(big("single", "单页", IconName::File).selected(!double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = false)
                    })))
                    .child(big("double", "双页", IconName::Columns2).selected(double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = true)
                    }))),
                None,
            ))
            .child(group(
                cx,
                "zoom",
                "缩放",
                h_flex()
                    .gap_1()
                    .child(big("z100", "100%", IconName::ZoomIn).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.doc.update(cx, |d, cx| d.set_zoom(doc_view::ZOOM_100, cx))
                    })))
                    .child(stack([
                        small("zfit", "页宽", IconName::StretchHorizontal)
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.doc.update(cx, |d, cx| d.fit_width(cx))))
                            .into_any_element(),
                        small("zin", "放大", IconName::ZoomIn).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.zoom_by(1.1, cx))).into_any_element(),
                        small("zout", "缩小", IconName::ZoomOut).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.zoom_by(1.0 / 1.1, cx))).into_any_element(),
                    ])),
                None,
            ))
            .child(group(
                cx,
                "show",
                "显示",
                stack([
                    Checkbox::new("guides")
                        .small()
                        .label("模板格线")
                        .checked(show_guides)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.ui.show_guides = v);
                        }))
                        .into_any_element(),
                    Checkbox::new("line-numbers")
                        .small()
                        .label("行号")
                        .checked(ui.line_numbers)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.ui.line_numbers = v);
                        }))
                        .into_any_element(),
                    Checkbox::new("page-numbers")
                        .small()
                        .label("页码")
                        .checked(ui.page_numbers)
                        .on_click(cx.listener(|t, v: &bool, _w, cx| {
                            let v = *v;
                            t.edit_settings(cx, |st| st.settings.ui.page_numbers = v);
                        }))
                        .into_any_element(),
                ]),
                None,
            ))
            .child(group(
                cx,
                "look",
                "外观",
                stack([Checkbox::new("dark")
                    .small()
                    .label("深色模式")
                    .checked(dark)
                    .on_click(|v: &bool, window, cx| {
                        let v = *v;
                        ribbon::set_theme(v, Some(window), cx);
                        let st = cx.global_mut::<AppState>();
                        st.settings.ui.dark = v;
                        st.save_settings();
                    })
                    .into_any_element()]),
                None,
            ))
            .into_any_element()
    }
}
