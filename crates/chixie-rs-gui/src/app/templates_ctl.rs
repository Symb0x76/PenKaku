//! The template library in the app: the folders (watched), 导入 / 导出, and the 文件 › 新建
//! gallery (Word's File › New: tabs, tags, sort, search, cards with live previews, 收藏).

use std::path::PathBuf;

use chixie_rs_core::font::library::{self as fontlib, LibraryWatcher};
use chixie_rs_template::{Template, TemplateStore};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::state::AppState;
use crate::templates_view::preview;

/// ChiXie3's 标签 for 写字模板.
pub const TAGS: [&str; 9] = ["全部模板", "人物角色", "教育学习", "艺术结构", "建筑风景", "动物植物", "科技科学", "交通玩具", "其他模板"];

/// Gallery state (文件 › 新建).
pub struct Gallery {
    /// 0 在线模板, 1 我的模板, 2 我的收藏.
    pub tab: usize,
    /// Index into [`TAGS`] (0 = all).
    pub tag: usize,
    /// 0 时间最新, 1 收藏优先, 2 名称.
    pub sort: usize,
    pub search: Entity<InputState>,
}

impl Gallery {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self { tab: 1, tag: 0, sort: 0, search: cx.new(|cx| InputState::new(window, cx).placeholder("请输入关键字")) }
    }
}

/// Keeps the template folders watched while the app runs.
pub struct TemplateWatch {
    _watcher: LibraryWatcher,
    _task: Task<()>,
}

/// The templates the gallery shows, in order: tab, tag, search and sort applied.
pub fn gallery_list(tab: usize, tag: usize, sort: usize, query: &str, cx: &App) -> Vec<Template> {
    let st = cx.global::<AppState>();
    let entries = st.template_lib.scan();
    let mut v: Vec<(Template, std::time::SystemTime)> = st
        .templates
        .templates
        .iter()
        .map(|t| (t.clone(), entries.iter().find(|e| e.template.name == t.name).map_or(std::time::SystemTime::UNIX_EPOCH, |e| e.modified)))
        .filter(|(t, _)| tab != 2 || t.favourite)
        .filter(|(t, _)| tag == 0 || t.tags.iter().any(|x| x == TAGS[tag]))
        .filter(|(t, _)| query.is_empty() || t.name.contains(query) || t.tags.iter().any(|x| x.contains(query)))
        .collect();
    match sort {
        0 => v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name))),
        1 => v.sort_by(|a, b| b.0.favourite.cmp(&a.0.favourite).then_with(|| a.0.name.cmp(&b.0.name))),
        _ => v.sort_by(|a, b| a.0.name.cmp(&b.0.name)),
    }
    v.into_iter().map(|x| x.0).collect()
}

impl ChixieRsApp {
    /// Watch the template folders; a change rescans them.
    pub(crate) fn watch_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirs = cx.global::<AppState>().template_lib.dirs();
        for d in &dirs {
            let _ = std::fs::create_dir_all(d);
        }
        let (tx, rx) = async_channel::unbounded::<()>();
        let watcher = match fontlib::watch(&dirs, move || {
            let _ = tx.try_send(());
        }) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("watching template folders: {e}");
                self.template_watch = None;
                return;
            }
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while rx.recv().await.is_ok() {
                // A burst of events (an editor saving) settles first.
                cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                while rx.try_recv().is_ok() {}
                if this.update_in(cx, |app, window, cx| app.templates_changed(window, cx)).is_err() {
                    break;
                }
            }
        });
        self.template_watch = Some(TemplateWatch { _watcher: watcher, _task: task });
    }

    /// Files in a template folder changed: reload the list (unless a template is being edited).
    pub(crate) fn templates_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.templates.read(cx).is_dirty() {
            return;
        }
        let scanned: Vec<Template> = cx.global::<AppState>().template_lib.scan().into_iter().map(|e| e.template).collect();
        if scanned != cx.global::<AppState>().templates.templates && !scanned.is_empty() {
            cx.global_mut::<AppState>().templates = TemplateStore { templates: scanned };
            self.templates.update(cx, |v, cx| v.reload(window, cx));
            cx.notify();
        }
    }

    /// 选项 › 模板 › 更改…: move the default folder (templates move with it).
    pub(crate) fn change_template_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("选择模板文件夹".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |app, window, cx| match cx.global::<AppState>().template_lib.move_default(&dir) {
                Ok(lib) => {
                    let new_dir = lib.default_dir.clone();
                    cx.global_mut::<AppState>().template_lib = lib;
                    app.edit_settings(cx, |st| st.settings.templates.library_dir = Some(new_dir.clone()));
                    window.push_notification(Notification::success(format!("模板文件夹已移到 {}", new_dir.display())), cx);
                    app.watch_templates(window, cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("移动失败，模板仍在原文件夹：{e}")), cx),
            });
        })
        .detach();
    }

    /// 选项 › 模板 › 添加…: one more (read-only) folder to read templates from.
    pub(crate) fn add_template_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("添加模板文件夹".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = this.update_in(cx, |app, window, cx| {
                app.edit_settings(cx, |st| {
                    for p in paths {
                        if !st.settings.templates.extra_dirs.contains(&p) {
                            st.settings.templates.extra_dirs.push(p);
                        }
                    }
                });
                app.apply_template_dirs(window, cx);
            });
        })
        .detach();
    }

    pub(crate) fn remove_template_dir(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_settings(cx, |st| {
            if ix < st.settings.templates.extra_dirs.len() {
                st.settings.templates.extra_dirs.remove(ix);
            }
        });
        self.apply_template_dirs(window, cx);
    }

    fn apply_template_dirs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = cx.global_mut::<AppState>();
        st.template_lib.extra_dirs = st.settings.templates.extra_dirs.clone();
        self.watch_templates(window, cx);
        self.templates_changed(window, cx);
    }

    pub(crate) fn open_template_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir: PathBuf = cx.global::<AppState>().template_lib.default_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = super::fonts_ctl::open_dir(&dir) {
            window.push_notification(Notification::error(format!("无法打开文件夹：{e}")), cx);
        }
    }

    /// 收藏 / 取消收藏 a template (saved to its file).
    pub(crate) fn toggle_favourite(&mut self, name: &str, cx: &mut Context<Self>) {
        let st = cx.global_mut::<AppState>();
        if let Some(t) = st.templates.templates.iter_mut().find(|t| t.name == name) {
            t.favourite = !t.favourite;
        }
        st.save_templates();
        cx.notify();
    }

    /// 文件 › 新建: the template gallery.
    #[inline(never)]
    pub(super) fn bs_gallery(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (border, hover, muted, accent, warn) = (theme.border, theme.secondary, theme.muted_foreground, theme.primary, theme.warning);
        let g = &self.gallery;
        let query = g.search.read(cx).value().trim().to_string();
        let tabs = TabBar::new("gal-tab")
            .underline()
            .selected_index(g.tab)
            .children([Tab::new().label("在线模板"), Tab::new().label("我的模板"), Tab::new().label("我的收藏")])
            .on_click(cx.listener(|a, ix: &usize, _w, cx| {
                a.gallery.tab = *ix;
                cx.notify();
            }));
        if g.tab == 0 {
            return v_flex()
                .gap_3()
                .child(tabs)
                .child(div().p_6().text_sm().text_color(muted).child("在线模板需要服务器支持，暂不可用。可以用「我的模板」里的模板，或在「模板管理」中导入别人分享的模板文件（.svg / .zip / .xlsx）。"))
                .into_any_element();
        }
        let chip = |i: usize, label: &'static str, on: bool, cx: &mut Context<Self>| {
            Button::new(("gal-tag", i)).xsmall().label(label).when(on, |b| b.primary()).when(!on, |b| b.ghost()).on_click(cx.listener(move |a, _: &ClickEvent, _w, cx| {
                a.gallery.tag = i;
                cx.notify();
            }))
        };
        let tags = h_flex().gap_1().flex_wrap().child(div().text_sm().text_color(muted).child("标签：")).children(TAGS.iter().enumerate().map(|(i, t)| chip(i, t, i == g.tag, cx)));
        let sort = h_flex().gap_1().child(div().text_sm().text_color(muted).child("排序：")).children(["时间最新", "收藏优先", "名称"].iter().enumerate().map(|(i, s)| {
            Button::new(("gal-sort", i)).xsmall().label(*s).when(i == g.sort, |b| b.outline()).when(i != g.sort, |b| b.ghost()).on_click(cx.listener(move |a, _: &ClickEvent, _w, cx| {
                a.gallery.sort = i;
                cx.notify();
            }))
        }));
        let card = |id: usize, name: String, kind: &'static str, t: Option<Template>| {
            v_flex()
                .id(("bs-new", id))
                .w(px(150.))
                .gap_1()
                .p_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|s| s.bg(hover))
                .child(div().w(px(134.)).h(px(176.)).border_1().border_color(border).bg(gpui_kit::white()).child(preview(t, gpui_kit::white(), border)))
                .child(div().text_sm().text_center().truncate().child(name))
                .child(div().text_xs().text_center().text_color(muted).child(kind))
        };
        let list = gallery_list(g.tab, g.tag, g.sort, &query, cx);
        let empty = list.is_empty();
        let mut row = h_flex().gap_4().flex_wrap();
        if g.tab == 1 && g.tag == 0 && query.is_empty() {
            row = row.child(card(0, "空白文档".into(), "", None).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.new_doc(Some(None), w, cx))));
        }
        for (i, t) in list.into_iter().enumerate() {
            let (name, fav, kind) = (t.name.clone(), t.favourite, t.kind_label());
            let star_name = name.clone();
            let star = div()
                .id(("gal-star", i))
                .absolute()
                .top(px(10.))
                .right(px(12.))
                .cursor_pointer()
                .child(Icon::new(IconName::Star).with_size(px(16.)).text_color(if fav { warn } else { muted.opacity(0.5) }))
                .on_click(cx.listener(move |a, _: &ClickEvent, _w, cx| {
                    cx.stop_propagation();
                    a.toggle_favourite(&star_name, cx);
                }));
            row = row.child(div().relative().child(card(i + 1, name.clone(), kind, Some(t)).on_click(cx.listener(move |a, _: &ClickEvent, w, cx| a.new_doc(Some(Some(name.clone())), w, cx)))).child(star));
        }
        v_flex()
            .gap_3()
            .child(tabs)
            .child(h_flex().gap_4().child(div().w(px(240.)).child(Input::new(&g.search).small())).child(sort))
            .child(tags)
            .child(div().text_xs().text_color(muted).child("点卡片新建文档（当前文档会被清空，草稿已自动保存）；点 ☆ 收藏。"))
            .child(row)
            .when(empty, |el| el.child(div().text_sm().text_color(accent).child(if g.tab == 2 { "还没有收藏的模板" } else { "没有符合条件的模板" })))
            .into_any_element()
    }

    /// 选项 › 模板: the library folders.
    #[inline(never)]
    pub(super) fn bs_template_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let default_dir = st.template_lib.default_dir.display().to_string();
        let extra_dirs = st.settings.templates.extra_dirs.clone();
        let muted = cx.theme().muted_foreground;
        let mut extra = v_flex().gap_1();
        for (i, d) in extra_dirs.iter().enumerate() {
            extra = extra.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(d.display().to_string()))
                    .child(Button::new(("opt-tpl-rmdir", i)).ghost().xsmall().label("移除").on_click(cx.listener(move |t, _: &ClickEvent, w, cx| t.remove_template_dir(i, w, cx)))),
            );
        }
        v_flex()
            .gap_2()
            .child(div().pt_2().text_base().font_weight(FontWeight::SEMIBOLD).child("模板"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("模板文件夹："))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(default_dir))
                    .child(Button::new("opt-tpl-open").small().ghost().label("打开").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_template_folder(w, cx))))
                    .child(Button::new("opt-tpl-move").small().label("更改…").tooltip("模板文件会一起移过去").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.change_template_dir(w, cx)))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("其他模板文件夹（只读：共享盘、U 盘）："))
                    .child(Button::new("opt-tpl-add").small().label("添加…").on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.add_template_dir(w, cx)))),
            )
            .child(extra)
            .child(div().text_xs().text_color(muted).child("每个模板是一个 SVG 文件（可直接用看图软件打开）；放进这些文件夹即可使用，无需重启。"))
            .into_any_element()
    }
}
