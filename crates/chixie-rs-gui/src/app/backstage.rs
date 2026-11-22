//! 文件 Backstage (Word's File view): a full-window page with a coloured navigation column
//! (← 返回, 新建, 打开, 保存, 另存为, 导出, 模板管理, 账户, 选项) and the chosen page beside it.
//! Esc or ← returns to the document.

use chixie_rs_core::session::Command;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, ThemeMode, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    New,
    Open,
    Export,
    Templates,
    Account,
    Options,
}

/// Navigation entries; `None` stage = an action (保存 / 另存为) that runs at once.
const NAV: [(&str, IconName, Option<Stage>); 8] = [
    ("新建", IconName::FilePlus, Some(Stage::New)),
    ("打开", IconName::FolderOpen, Some(Stage::Open)),
    ("保存", IconName::Save, None),
    ("另存为", IconName::Copy, None),
    ("导出", IconName::FileDown, Some(Stage::Export)),
    ("模板管理", IconName::LayoutGrid, Some(Stage::Templates)),
    ("账户", IconName::User, Some(Stage::Account)),
    ("选项", IconName::Settings, Some(Stage::Options)),
];

impl ChixieRsApp {
    pub(super) fn render_backstage(&mut self, stage: Stage, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (bg, fg, nav_bg, nav_fg) = (theme.background, theme.foreground, theme.primary, theme.primary_foreground);
        let nav_hover = theme.primary_hover;
        let nav_sel = nav_fg.opacity(0.18);
        let items = NAV.iter().enumerate().map(|(i, (label, icon, target))| {
            let (target, label) = (*target, *label);
            h_flex()
                .id(("bs-nav", i))
                .h(px(40.))
                .px_4()
                .gap_3()
                .cursor_pointer()
                .text_color(nav_fg)
                .when(target == Some(stage), |el| el.bg(nav_sel))
                .hover(|s| s.bg(nav_hover))
                .child(Icon::new(*icon).with_size(px(18.)))
                .child(label)
                .on_click(cx.listener(move |t, _: &ClickEvent, w, cx| match target {
                    Some(s) => t.open_backstage(s, w, cx),
                    None => t.save_doc(label == "另存为", w, cx),
                }))
        });
        let nav = v_flex()
            .w(px(200.))
            .h_full()
            .bg(nav_bg)
            .py_3()
            .child(
                h_flex()
                    .id("bs-back")
                    .h(px(48.))
                    .px_4()
                    .cursor_pointer()
                    .text_color(nav_fg)
                    .hover(|s| s.bg(nav_hover))
                    .child(Icon::new(IconName::ArrowLeft).with_size(px(22.)))
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.close_backstage(w, cx))),
            )
            .children(items);
        let (title, body) = match stage {
            Stage::New => ("新建", self.bs_gallery(cx)),
            Stage::Open => ("打开", self.bs_open(cx)),
            Stage::Export => ("导出", self.bs_export(cx)),
            Stage::Templates => ("模板管理", self.templates.clone().into_any_element()),
            Stage::Account => ("账户", self.bs_account(cx)),
            Stage::Options => ("选项", self.bs_options(cx)),
        };
        h_flex()
            .id("backstage")
            .track_focus(&self.backstage_focus)
            .on_key_down(cx.listener(|t, e: &KeyDownEvent, w, cx| {
                if e.keystroke.key == "escape" {
                    t.close_backstage(w, cx);
                }
            }))
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(bg)
            .text_color(fg)
            .child(nav)
            .child(
                v_flex()
                    .id("bs-content")
                    .flex_1()
                    .h_full()
                    .px_10()
                    .py_8()
                    .gap_6()
                    .overflow_y_scroll()
                    .child(div().text_3xl().child(title))
                    .child(body),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn bs_open(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child("打开 Word 文档（.docx；chixie-rs 保存的文档保留每个字的笔迹）、旧版 .chx 或纯文本（.txt）。"))
            .child(
                Button::new("bs-browse")
                    .large()
                    .icon(Icon::new(IconName::FolderOpen))
                    .label("浏览…")
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_doc(w, cx))),
            )
            .child(div().pt_4().text_sm().text_color(muted).child("把文件中的文字导入当前文档（txt、docx、pdf、xlsx；表格模板可导入 xlsx 填表）。也可以把文件拖到页面上。"))
            .child(
                Button::new("bs-import")
                    .large()
                    .icon(Icon::new(IconName::FileInput))
                    .label("导入到当前文档…")
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| {
                        t.close_backstage(w, cx);
                        crate::import_ui::pick_and_import(t.doc.clone(), w, cx);
                    })),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn bs_export(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let doc = self.doc.read(cx).document();
        let extent = doc.bounds().map(|(x0, y0, x1, y1)| format!("书写范围 {:.1} × {:.1} mm", x1 - x0, y1 - y0)).unwrap_or_else(|| "无笔画".into());
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child("把文档导出为 G-code，每页一个文件（可用于 SD 卡或其他上位机）。"))
            .child(div().text_sm().child(format!("共 {} 页 · {} 笔 · {extent}", doc.pages.len(), doc.stroke_count())))
            .child(
                Button::new("bs-export")
                    .large()
                    .primary()
                    .icon(Icon::new(IconName::FileDown))
                    .label("导出 G-code…")
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.export(w, cx))),
            )
            .child(div().pt_4().text_sm().text_color(muted).child("导出 PDF 或打印：按实际尺寸输出，可打印笔迹，也可只打印模板框线（自制作文纸、表格）。"))
            .child({
                let ui = cx.global::<AppState>().settings.ui.clone();
                h_flex()
                    .gap_4()
                    .child(Checkbox::new("pdf-text").label("打印文字").checked(ui.print_text).on_click(cx.listener(|t, v: &bool, _w, cx| {
                        let v = *v;
                        t.edit_settings(cx, |st| st.settings.ui.print_text = v);
                    })))
                    .child(Checkbox::new("pdf-bg").label("打印背景图").checked(ui.print_background).on_click(cx.listener(|t, v: &bool, _w, cx| {
                        let v = *v;
                        t.edit_settings(cx, |st| st.settings.ui.print_background = v);
                    })))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("bs-pdf").large().icon(Icon::new(IconName::FileDown)).label("导出 PDF…").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.export_pdf(w, cx))))
                    .child(Button::new("bs-print").large().icon(Icon::new(IconName::Printer)).label("打印…").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.print_doc(w, cx)))),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn bs_account(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let font = st.active_font.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| "未选择".into());
        let Some(user) = st.user.as_ref().map(|u| format!("{}（{}）", u.name, u.role)) else {
            // Logged out: the login form. Only needed to download fonts.
            return v_flex()
                .gap_3()
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(format!(
                    "当前字体：{font}。无需登录也能使用全部功能；登录只用于下载云端字体，下载后的字体保存在本机。"
                )))
                .child(div().h(px(420.)).w(px(480.)).child(self.login.clone()))
                .into_any_element();
        };
        v_flex()
            .gap_3()
            .child(h_flex().gap_3().child(Icon::new(IconName::User).with_size(px(40.))).child(div().text_lg().child(user)))
            .child(div().text_sm().child(format!("当前字体：{font}")))
            .child(
                Button::new("bs-logout")
                    .danger()
                    .icon(Icon::new(IconName::LogOut))
                    .label("退出登录")
                    .on_click(|_: &ClickEvent, _w, cx| cx.global::<AppState>().session.send(Command::Logout)),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn bs_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let dark = cx.theme().mode == ThemeMode::Dark;
        let st = cx.global::<AppState>();
        let show_guides = st.settings.ui.show_guides;
        let muted = cx.theme().muted_foreground;
        let path = st.settings_path.display().to_string();
        v_flex()
            .gap_4()
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("常规"))
            .child(Checkbox::new("opt-dark").label("深色模式").checked(dark).on_click(|v: &bool, window, cx| {
                let v = *v;
                crate::ribbon::set_theme(v, Some(window), cx);
                let st = cx.global_mut::<AppState>();
                st.settings.ui.dark = v;
                st.save_settings();
            }))
            .child(Checkbox::new("opt-guides").label("在页面上显示模板格线").checked(show_guides).on_click(cx.listener(|t, v: &bool, _w, cx| {
                let v = *v;
                t.edit_settings(cx, |st| st.settings.ui.show_guides = v);
            })))
            .child(self.bs_font_options(cx))
            .child(self.bs_template_options(cx))
            .child(div().pt_2().text_base().font_weight(FontWeight::SEMIBOLD).child("从炽写迁移"))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("opt-style").icon(Icon::new(IconName::Import)).label("导入手写效果（config.ini）…").on_click(cx.listener(
                        |t, _: &ClickEvent, w, cx| t.import_style(w, cx),
                    )))
                    .child(Button::new("opt-tpl").icon(Icon::new(IconName::LayoutGrid)).label("导入模板（.cx）…").on_click(cx.listener(
                        |t, _: &ClickEvent, w, cx| t.open_backstage(super::Stage::Templates, w, cx),
                    ))),
            )
            .child(div().text_xs().text_color(muted).child(format!("设置文件：{path}")))
            .into_any_element()
    }

    /// 选项 › 字体: the library folders and 字体优化.
    #[inline(never)]
    fn bs_font_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let fonts = st.settings.fonts.clone();
        let default_dir = st.fonts.dir().display().to_string();
        let muted = cx.theme().muted_foreground;
        let fix = fonts.fix;
        let fix_box = |id: &'static str, label: &'static str, on: bool, set: fn(&mut chixie_rs_core::font::fix::FontFix, bool)| {
            Checkbox::new(id).label(label).checked(on).on_click(cx.listener(move |t, v: &bool, w, cx| {
                let mut f = cx.global::<AppState>().settings.fonts.fix;
                set(&mut f, *v);
                t.set_font_fix(f, w, cx);
            }))
        };
        let mut extra = v_flex().gap_1();
        for (i, d) in fonts.extra_dirs.iter().enumerate() {
            extra = extra.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(d.display().to_string()))
                    .child(Button::new(("opt-rmdir", i)).ghost().xsmall().label("移除").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.remove_library_dir(i, w, cx)))),
            );
        }
        v_flex()
            .gap_2()
            .child(div().pt_2().text_base().font_weight(FontWeight::SEMIBOLD).child("字体"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("字体文件夹："))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(default_dir))
                    .child(Button::new("opt-movedir").small().label("更改…").tooltip("字体文件会一起移过去").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.change_library_dir(w, cx)))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("其他字体文件夹（共享盘、U 盘）："))
                    .child(Button::new("opt-adddir").small().label("添加…").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.add_library_dir(w, cx)))),
            )
            .child(extra)
            .child(div().text_xs().text_color(muted).child("每个字体是一个 SVG 文件；放进这些文件夹即可使用，无需重启。"))
            .child(div().pt_1().text_sm().child("字体优化（读取字体时应用，不改动文件）："))
            .child(
                h_flex()
                    .gap_4()
                    .child(fix_box("opt-fix-size", "统一字符尺寸", fix.unify_size, |f, v| f.unify_size = v))
                    .child(fix_box("opt-fix-dedupe", "重复轨迹优化", fix.dedupe, |f, v| f.dedupe = v))
                    .child(fix_box("opt-fix-width", "异常宽度优化", fix.widths, |f, v| f.widths = v)),
            )
            .into_any_element()
    }
}
