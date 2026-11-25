//! Main window, laid out like Microsoft Word: login start screen, then a tab strip with a quick
//! access toolbar and a filled 文件 tab (opens the Backstage), the ribbon (开始 / 布局 / 手写 /
//! 设备 / 视图), the page editor with the device task pane, and a status bar.
//! Session and device events are pumped here and fanned out to views + toasts.

mod backstage;
mod print;
pub(crate) mod menu;
pub(crate) mod fonts_ctl;
mod resume;
mod sd;
pub(crate) mod templates_ctl;
pub(crate) mod tune;
mod ribbon_tabs;

use std::path::PathBuf;
use std::time::Duration;

use chixie_rs_core::config::import_chixie3;
use chixie_rs_device::device::{Command as DevCommand, DeviceHandle, Event as DevEvent, Job, JobOutcome};
use chixie_rs_device::job::{self, PatrolMode};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use chixie_rs_doc::EditDoc;
use chixie_rs_output as gcode;
use chixie_rs_core::session::{Command, Event, FontListKind};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, Selectable as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub use backstage::Stage;
pub use crate::nav_pane::NavTab as NavTabPub;

use crate::device::{DevicePanel, Notice};
use crate::doc_view::{self, DocView};
use crate::file_ops;
use crate::font_preview::{FontItem, FontPreview};
use crate::fonts::{FontsEvent, FontsView};
use crate::login::LoginView;
use crate::ribbon::{self, RIBBON_H};
use crate::state::{ActiveFont, AppState, User};
use crate::style_dialog::StyleDialog;
use crate::templates_view::{TemplateEvent, TemplatesView};

pub const TABS: [&str; 5] = ["开始", "布局", "手写", "设备", "视图"];
pub const TAB_DEVICE: usize = 3;
const DRAFT_EVERY: Duration = Duration::from_secs(2);
/// Plotter travel on the verified board ($130/$131 = 300 mm).
const TRAVEL_MM: f32 = 300.0;

/// 工作区巡边 / 绘制框架线 on the caret's page.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageTool {
    /// Mode, and range: `true` = 模板框线区域, `false` = 实际书写区域.
    Patrol(PatrolMode, bool),
    Ruling,
}

pub struct ChixieRsApp {
    pub tab: usize,
    pub login: Entity<LoginView>,
    pub fonts: Entity<FontsView>,
    pub doc: Entity<DocView>,
    pub device: Entity<DevicePanel>,
    pub templates: Entity<TemplatesView>,
    /// Right-hand device task pane (opens on connect / job start).
    pub show_pane: bool,
    /// Full-window 文件 view; `None` shows the document.
    pub backstage: Option<Stage>,
    ribbon_collapsed: bool,
    font_size: Entity<InputState>,
    line_height: Entity<InputState>,
    font_select: Entity<SelectState<Vec<FontItem>>>,
    /// Font ids, parallel to the font select's items.
    font_ids: Vec<i64>,
    /// What the font / size boxes last showed (selection?, font, size): see `sync_format_boxes`.
    format_shown: Option<(bool, Option<Option<String>>, Option<f32>)>,
    /// Local fonts whose preview is being built in the background.
    previews_pending: std::collections::HashSet<i64>,
    style_dialog: Entity<StyleDialog>,
    backstage_focus: FocusHandle,
    /// Where 保存 writes; `None` until 另存为, or 打开 of one of our documents (`.chx` saves as `.docx`).
    file: Option<PathBuf>,
    zoom_slider: Entity<SliderState>,
    /// Last title pushed to the OS window (taskbar).
    win_title: String,
    /// 断点续写 card and the break point of the last interrupted job.
    pub(crate) resume: resume::Resume,
    /// Page of the running 书写 job (not 巡边 / 框线): when it ends done, the next page is
    /// selected and the user is asked to change the sheet.
    writing: Option<usize>,
    /// 换纸 card: the page just written and the next one (selected, waiting for 书写).
    pub(crate) sheet_change: Option<(usize, usize)>,
    /// The 字符微调 card is shown in the task pane.
    tune_open: bool,
    /// 查找 / 替换 bar above the page.
    find_bar: Entity<crate::find_bar::FindBar>,
    find_open: bool,
    /// 导航 pane (搜索 / 缺字) left of the page.
    nav: Entity<crate::nav_pane::NavPane>,
    nav_open: bool,
    /// Library folders watched for added / removed fonts.
    library_watch: Option<fonts_ctl::LibraryWatch>,
    /// Template folders watched for added / changed / removed templates.
    template_watch: Option<templates_ctl::TemplateWatch>,
    /// 文件 › 新建 gallery filters.
    pub(crate) gallery: templates_ctl::Gallery,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

/// A device job from page bodies (resume maps and pen stops included).
pub(crate) fn job_of(first_page: usize, codes: Vec<gcode::PageCode>) -> Job {
    let mut job = Job { first_page, ..Default::default() };
    for c in codes {
        job.pages.push(c.text);
        job.maps.push(c.stroke_lines);
        job.pens.push(c.pens);
    }
    job
}

impl ChixieRsApp {
    /// Every page as it is written (路径规划, 页面旋转, 书写原点 applied); the editor shows the
    /// layout itself.
    pub(crate) fn written_pages(&self, cx: &App) -> Vec<gcode::Written> {
        let settings = &cx.global::<AppState>().settings;
        let opts = gcode::OutputOptions::from_settings(settings);
        let view = self.doc.read(cx);
        let d = view.doc().document();
        let colours: Vec<_> = (0..view.doc().len()).map(|i| d.attrs(i).colour).collect();
        gcode::plan_all(&view.document(), &colours, settings.page.multicolour, &opts)
    }

    pub fn new(
        events: async_channel::Receiver<Event>,
        device: DeviceHandle,
        device_events: async_channel::Receiver<DevEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let login = cx.new(|cx| LoginView::new(window, cx));
        let fonts = cx.new(|cx| FontsView::new(window, cx));
        let initial = file_ops::open_draft(&cx.global::<AppState>().settings_path).map(|l| l.doc).unwrap_or_else(|_| EditDoc::from_text("你好，chixie-rs！\n春眠不觉晓，处处闻啼鸟。"));
        let doc = cx.new(|cx| DocView::new(initial, window, cx));
        let find_bar = cx.new(|cx| crate::find_bar::FindBar::new(doc.clone(), window, cx));
        let nav = cx.new(|cx| crate::nav_pane::NavPane::new(doc.clone(), window, cx));
        let page = cx.global::<AppState>().settings.page.clone();
        let font_size = cx.new(|cx| InputState::new(window, cx).step(5.0).min(5.0).max(1000.0).default_value(fmt_num(page.font_size)));
        let line_height = cx.new(|cx| InputState::new(window, cx).step(0.25).min(1.0).max(200.0).default_value(fmt_num(page.line_height)));
        let font_select = cx.new(|cx| SelectState::new(Vec::<FontItem>::new(), None, window, cx));
        let templates = cx.new(|cx| TemplatesView::new(window, cx));
        let style_dialog = cx.new(|cx| StyleDialog::new(window, cx));
        let device = cx.new(|cx| DevicePanel::new(device, window, cx));
        let zoom_slider = cx.new(|_| SliderState::new().min(0.0).max(100.0).step(0.5).default_value(50.0));
        let gallery = templates_ctl::Gallery::new(window, cx);
        let gallery_search = gallery.search.clone();

        let pump = cx.spawn_in(window, async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update_in(cx, |app, window, cx| app.on_session_event(ev, window, cx)).is_err() {
                    break;
                }
            }
        });
        let dev_pump = cx.spawn_in(window, async move |this, cx| {
            while let Ok(ev) = device_events.recv().await {
                if this.update_in(cx, |app, window, cx| app.on_device_event(ev, window, cx)).is_err() {
                    break;
                }
            }
        });
        // Draft autosave: cheap poll of the editor's dirty flag.
        let autosave = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAFT_EVERY).await;
                let Ok(()) = this.update(cx, |app, cx| app.save_draft(cx)) else { break };
            }
        });
        let subs = vec![
            cx.observe_global::<AppState>(|_, cx| cx.notify()),
            cx.observe_in(&doc, window, |this, _, window, cx| {
                this.sync_zoom_slider(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&zoom_slider, window, |this, _s, ev: &SliderEvent, _w, cx| {
                if let SliderEvent::Change(v) = ev {
                    let z = slider_to_zoom(v.start()) / 100.0 * doc_view::ZOOM_100;
                    this.doc.update(cx, |d, cx| d.set_zoom(z, cx));
                }
            }),
            cx.observe(&device, |_, _, cx| cx.notify()),
            cx.subscribe_in(&gallery_search, window, |_this, _s, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            // The 断点续写 card shows the selected character; the font and size boxes show the
            // selection's.
            cx.observe_in(&doc, window, |t, _, window, cx| {
                if t.resume.open {
                    cx.notify();
                }
                t.sync_format_boxes(window, cx);
            }),
            cx.subscribe_in(&font_size, window, |this, s, ev: &InputEvent, _w, cx| {
                if let (InputEvent::Change, Some(v)) = (ev, parse_in(s, cx, 5.0, 1000.0)) {
                    if this.doc.read(cx).doc().selection().is_empty() {
                        this.edit_settings(cx, |st| st.settings.page.font_size = v);
                    } else {
                        this.doc.update(cx, |d, cx| d.set_run_size(v, cx));
                    }
                }
            }),
            cx.subscribe_in(&line_height, window, |this, s, ev: &InputEvent, _w, cx| {
                if let (InputEvent::Change, Some(v)) = (ev, parse_in(s, cx, 1.0, 200.0)) {
                    this.edit_settings(cx, |st| st.settings.page.line_height = v);
                }
            }),
            cx.subscribe_in(&font_select, window, |this, _s, ev: &SelectEvent<Vec<FontItem>>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = ev {
                    this.choose_font(*id, window, cx);
                }
            }),
            cx.subscribe_in(&fonts, window, |this, _v, ev: &FontsEvent, window, cx| match ev {
                FontsEvent::UseLocal(id) => this.use_main_font(*id, window, cx),
                FontsEvent::RemoveLocal(id) => this.remove_local_font(*id, window, cx),
                FontsEvent::SetRole(id, role) => this.set_role(*id, *role, window, cx),
                FontsEvent::ClearRole(role) => this.clear_role(*role, window, cx),
                FontsEvent::ToggleMix(id) => this.toggle_mix(*id, window, cx),
                FontsEvent::MixWeight(id, w) => this.set_mix_weight(*id, *w, window, cx),
                FontsEvent::RerollMix => this.reroll_mix(window, cx),
                FontsEvent::Reveal(id) => this.reveal_font(*id, window, cx),
                FontsEvent::Export(id) => this.export_font(*id, window, cx),
                FontsEvent::Import => this.import_fonts(window, cx),
                FontsEvent::OpenFolder => this.open_font_folder(window, cx),
                FontsEvent::Login => {
                    window.close_dialog(cx);
                    this.open_backstage(Stage::Account, window, cx);
                }
            }),
            cx.subscribe(&find_bar, |this, _b, ev: &crate::find_bar::FindBarEvent, cx| match ev {
                crate::find_bar::FindBarEvent::Close => {
                    this.find_open = false;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&nav, window, |this, _p, ev: &crate::nav_pane::NavPaneEvent, window, cx| match ev {
                crate::nav_pane::NavPaneEvent::Close => this.close_nav(window, cx),
            }),
            cx.subscribe_in(&templates, window, |this, _v, ev: &TemplateEvent, window, cx| match ev {
                TemplateEvent::Apply(name) => {
                    this.apply_template(Some(name.clone()), cx);
                    this.close_backstage(window, cx);
                }
            }),
        ];
        // No login needed: list the library, load the fonts in use, and watch the folders.
        cx.defer_in(window, |this, window, cx| {
            this.sync_font_select(window, cx);
            this.reload_fonts(window, cx);
            this.watch_library(window, cx);
            this.watch_templates(window, cx);
        });
        Self {
            tab: 0,
            login,
            fonts,
            doc,
            device,
            templates,
            show_pane: false,
            backstage: None,
            ribbon_collapsed: false,
            font_size,
            line_height,
            font_select,
            font_ids: Vec::new(),
            format_shown: None,
            previews_pending: Default::default(),
            style_dialog,
            backstage_focus: cx.focus_handle(),
            file: None,
            zoom_slider,
            win_title: String::new(),
            resume: Default::default(),
            writing: None,
            sheet_change: None,
            tune_open: false,
            find_bar,
            find_open: false,
            nav,
            nav_open: false,
            library_watch: None,
            template_watch: None,
            gallery,
            _tasks: vec![pump, dev_pump, autosave],
            _subs: subs,
        }
    }

    fn on_session_event(&mut self, ev: Event, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            Event::LoggedIn { username, role } => {
                self.login.update(cx, |l, cx| l.on_logged_in(cx));
                let st = cx.global_mut::<AppState>();
                st.user = Some(User { name: username.clone(), role });
                // The last-used font is normally already loaded from the local store; fetch it only
                // if it was never downloaded.
                if st.active_font.is_none()
                    && let Some(id) = st.settings.fonts.roles.main.as_ref().and_then(|k| k.cloud_id()).filter(|id| !st.fonts.contains(*id))
                {
                    st.session.send(Command::LoadFont { id });
                }
                st.session.send(Command::CollectedFonts);
                window.push_notification(Notification::success(format!("欢迎，{username}")), cx);
                if self.backstage == Some(Stage::Account) {
                    self.close_backstage(window, cx);
                } else {
                    self.focus_doc(window, cx);
                }
            }
            Event::LoginFailed(msg) => self.login.update(cx, |l, cx| l.on_failed(msg, cx)),
            // Logging out only affects cloud access: the document and downloaded fonts stay.
            Event::LoggedOut => {
                cx.global_mut::<AppState>().user = None;
                self.fonts.update(cx, |f, cx| f.clear_cloud(cx));
                self.sync_font_select(window, cx);
                window.push_notification(Notification::info("已退出登录；本地字体仍可使用"), cx);
            }
            Event::SessionLost(msg) => {
                cx.global_mut::<AppState>().user = None;
                self.fonts.update(cx, |f, cx| f.clear_cloud(cx));
                self.sync_font_select(window, cx);
                window.push_notification(Notification::warning(format!("登录已失效（{msg}）；本地字体仍可使用")), cx);
            }
            Event::Fonts { kind, page, fonts } => {
                let collected = kind == FontListKind::Collected;
                // A font downloaded before the lists arrived was saved under a placeholder name;
                // the server's name wins.
                let renamed = self.repair_font_names(&fonts, cx);
                self.fonts.update(cx, |f, cx| f.set_list(kind, page, fonts, cx));
                if collected || renamed {
                    self.sync_font_select(window, cx);
                }
            }
            Event::FontLoaded { id, font } => {
                // The last font is fetched at login, often before the font lists arrive.
                let name = self
                    .fonts
                    .read(cx)
                    .name_of(id)
                    .or_else(|| cx.global::<AppState>().fonts.name_of(id).filter(|n| !is_placeholder_name(n)))
                    .unwrap_or_else(|| placeholder_name(id));
                if let Some(p) = FontPreview::new(&font, &name) {
                    cx.global_mut::<AppState>().previews.insert(id, std::sync::Arc::new(p));
                }
                self.fonts.update(cx, |f, cx| f.font_loaded(cx));
                let glyphs = font.glyphs.len();
                // Keep a copy on this computer so the font works without logging in next time.
                // Saved into the library folder as an SVG font; then it becomes the main font.
                let store = cx.global::<AppState>().fonts.clone();
                let (to_save, save_name) = (font.clone(), name.clone());
                let save = cx.background_executor().spawn(async move { store.save(id, &save_name, &to_save) });
                cx.spawn_in(window, async move |this, cx| {
                    let res = save.await;
                    let _ = this.update_in(cx, |app, window, cx| match res {
                        Ok(()) => app.set_role(id, fonts_ctl::Role::Main, window, cx),
                        Err(e) => window.push_notification(Notification::error(format!("保存字体失败：{e}")), cx),
                    });
                })
                .detach();
                // Usable right away, before the save finishes.
                let st = cx.global_mut::<AppState>();
                st.font_set = std::sync::Arc::new(chixie_rs_core::font::fontset::FontSet::single(font.clone()));
                st.font_set_gen += 1;
                st.active_font = Some(ActiveFont { id, name: name.clone(), font });
                if window.has_active_dialog(cx) {
                    window.close_dialog(cx);
                }
                self.sync_font_select(window, cx);
                window.push_notification(Notification::success(format!("已加载 {name}（{glyphs} 字）")), cx);
                self.focus_doc(window, cx);
            }
            Event::Error(msg) => {
                self.fonts.update(cx, |f, cx| f.font_loaded(cx));
                window.push_notification(Notification::error(msg), cx);
            }
        }
        cx.notify();
    }

    /// 替换: show the bar and focus its search box (查找 opens the 导航 pane instead).
    pub(crate) fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.backstage.is_some() {
            return;
        }
        self.find_open = true;
        self.find_bar.update(cx, |b, cx| b.open(replace, window, cx));
        cx.notify();
    }

    pub(crate) fn nav_open(&self) -> bool {
        self.nav_open
    }
    /// Matches the 搜索 tab lists (autotest).
    pub(crate) fn nav_hit_count(&self, cx: &App) -> usize {
        self.nav.read(cx).hit_count(cx)
    }
    pub(crate) fn close_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_open = false;
        self.focus_doc(window, cx);
        cx.notify();
    }

    /// 导航 pane on `tab` (Ctrl+F: 搜索; the status bar's 缺字: 缺字).
    pub(crate) fn open_nav(&mut self, tab: crate::nav_pane::NavTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.backstage.is_some() {
            return;
        }
        self.nav_open = true;
        self.nav.update(cx, |p, cx| p.open(tab, window, cx));
        cx.notify();
    }

    /// The font select and size box show the selection's font and size (blank when mixed), or
    /// the document's without a selection. Only touched when that changes.
    fn sync_format_boxes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fs = self.doc.read(cx).format_state(cx);
        let key = (fs.selected, fs.font.clone(), fs.size);
        if self.format_shown.as_ref() == Some(&key) {
            return;
        }
        self.format_shown = Some(key);
        let st = cx.global::<AppState>();
        let main = st.active_font.as_ref().map(|f| f.id);
        let (font, size) = if fs.selected {
            let font = match &fs.font {
                Some(Some(name)) => st.fonts.id_of(name),
                Some(None) => main,
                None => None,
            };
            (font, fs.size.map(fmt_num).unwrap_or_default())
        } else {
            (main, fmt_num(st.settings.page.font_size))
        };
        let selected = font.and_then(|id| self.font_ids.iter().position(|&x| x == id)).map(IndexPath::new);
        self.font_select.update(cx, |s, cx| s.set_selected_index(selected, window, cx));
        if self.font_size.read(cx).value().as_ref() != size {
            self.font_size.update(cx, |s, cx| s.set_value(size, window, cx));
        }
    }

    /// Font picker items = downloaded fonts, then collected fonts not yet downloaded (marked
    /// 云端, login needed); selection = the active font.
    fn sync_font_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        let local = st.fonts.list();
        let mut items: Vec<FontItem> = local
            .iter()
            .map(|f| FontItem { id: f.id, name: f.name.clone().into(), cloud: false })
            .collect();
        for f in self.fonts.read(cx).collected() {
            if !items.iter().any(|it| it.id == f.id) {
                items.push(FontItem { id: f.id, name: f.font_name.clone().into(), cloud: true });
            }
        }
        let missing: Vec<(i64, String)> =
            local.into_iter().filter(|f| !st.previews.contains_key(&f.id) && !self.previews_pending.contains(&f.id)).map(|f| (f.id, f.name)).collect();
        let active = st.active_font.as_ref().map(|f| f.id);
        self.font_ids = items.iter().map(|it| it.id).collect();
        let selected = active.and_then(|id| self.font_ids.iter().position(|&x| x == id)).map(IndexPath::new);
        self.font_select.update(cx, |s, cx| {
            s.set_items(items, window, cx);
            s.set_selected_index(selected, window, cx);
        });
        if !missing.is_empty() {
            self.build_previews(missing, window, cx);
        }
    }

    /// Read local fonts off the UI thread and keep only their small previews.
    fn build_previews(&mut self, fonts: Vec<(i64, String)>, window: &mut Window, cx: &mut Context<Self>) {
        self.previews_pending.extend(fonts.iter().map(|(id, _)| *id));
        let store = cx.global::<AppState>().fonts.clone();
        let task = cx.background_executor().spawn(async move {
            fonts
                .into_iter()
                .filter_map(|(id, name)| {
                    let font = store.load(id).inspect_err(|e| log::warn!("preview of font {id}: {e}")).ok()?;
                    FontPreview::new(&font, &name).map(|p| (id, std::sync::Arc::new(p)))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let built = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                for (id, p) in built {
                    app.previews_pending.remove(&id);
                    cx.global_mut::<AppState>().previews.insert(id, p);
                }
                app.sync_font_select(window, cx);
            });
        })
        .detach();
    }

    /// Rename locally saved fonts after the server's names; true if any changed.
    fn repair_font_names(&mut self, fonts: &[chixie_rs_core::api::FontInfo], cx: &mut Context<Self>) -> bool {
        let st = cx.global_mut::<AppState>();
        let mut changed = false;
        for f in fonts {
            match st.fonts.rename(f.id, &f.font_name) {
                Ok(true) => {
                    changed = true;
                    // The preview may have been a sample for want of the name.
                    st.previews.remove(&f.id);
                    if let Some(a) = st.active_font.as_mut().filter(|a| a.id == f.id) {
                        a.name = f.font_name.clone();
                    }
                }
                Ok(false) => {}
                Err(e) => log::warn!("renaming local font {}: {e}", f.id),
            }
        }
        changed
    }

    fn choose_font(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.doc.read(cx).doc().selection().is_empty() {
            // The selection's own font: a downloaded one; the main font = the document's.
            let st = cx.global::<AppState>();
            let main = st.active_font.as_ref().map(|f| f.id) == Some(id);
            match st.fonts.name_of(id) {
                _ if main => self.doc.update(cx, |d, cx| d.set_run_font(None, cx)),
                Some(name) => self.doc.update(cx, |d, cx| d.set_run_font(Some(name), cx)),
                None => window.push_notification(Notification::info("请先下载这个字体，再设为所选文字的字体"), cx),
            }
            return;
        }
        if cx.global::<AppState>().active_font.as_ref().map(|f| f.id) != Some(id) {
            self.use_font_id(id, window, cx);
        }
    }

    /// Use font `id`: from the local store if downloaded, else download it (login needed).
    pub fn use_font_id(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<AppState>().fonts.contains(id) {
            self.use_main_font(id, window, cx);
        } else if cx.global::<AppState>().user.is_some() {
            self.fonts.update(cx, |f, cx| f.use_font(id, cx));
        } else {
            window.push_notification(Notification::info("这个字体还没下载，请先登录"), cx);
            self.open_backstage(Stage::Account, window, cx);
        }
    }

    fn remove_local_font(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = cx.global::<AppState>().fonts.remove(id) {
            window.push_notification(Notification::error(format!("删除失败：{e}")), cx);
        }
        // A deleted font no longer plays a role or takes part in the mix.
        let key = chixie_rs_core::font::library::FontKey::from_local_id(id);
        self.edit_settings(cx, |st| {
            let f = &mut st.settings.fonts;
            for r in fonts_ctl::Role::ALL {
                if r != fonts_ctl::Role::Main && r.get(&f.roles) == Some(&key) {
                    *r.slot(&mut f.roles) = None;
                }
            }
            f.mix.retain(|m| m.key != key);
        });
        self.sync_font_select(window, cx);
        self.fonts.update(cx, |_, cx| cx.notify());
        self.reload_fonts(window, cx);
    }

    pub fn focus_doc(&self, window: &mut Window, cx: &mut App) {
        let focus = self.doc.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Apply a settings change and persist it; the editor re-lays out via its global observer.
    fn edit_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut AppState)) {
        let st = cx.global_mut::<AppState>();
        f(st);
        st.save_settings();
    }

    /// Use template `name` for the page (`None`: blank page, current paper kept).
    pub fn apply_template(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.edit_settings(cx, |st| match name.as_deref().and_then(|n| st.templates.get(n)).cloned() {
            Some(t) => t.apply_to(&mut st.settings.page),
            None => st.settings.page.template = None,
        });
    }

    /// Paper size / orientation / margin changes make the page a blank (template-less) page.
    fn edit_page(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut chixie_rs_core::config::PageSettings)) {
        self.edit_settings(cx, |st| {
            st.settings.page.template = None;
            f(&mut st.settings.page);
        });
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) {
        if !self.doc.update(cx, |d, _| d.take_dirty()) {
            return;
        }
        let st = cx.global::<AppState>();
        let path = file_ops::draft_path(&st.settings_path);
        if let Err(e) = file_ops::save(&path, self.doc.read(cx).doc(), st.settings.style.handwriting.seed, &file_ops::table_shapes(st.active_template().as_ref())) {
            log::warn!("draft {}: {e}", path.display());
        }
    }

    // ---- 文件 ----

    pub fn new_doc(&mut self, template: Option<Option<String>>, window: &mut Window, cx: &mut Context<Self>) {
        self.file = None;
        if let Some(t) = template {
            self.apply_template(t, cx);
        }
        self.doc.update(cx, |d, cx| d.replace_doc(EditDoc::from_text(""), cx));
        self.close_backstage(window, cx);
    }

    fn open_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("打开".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |app, window, cx| match file_ops::open(&path) {
                Ok(l) => {
                    if let Some(seed) = l.seed {
                        app.edit_settings(cx, |st| st.settings.style.handwriting.seed = seed);
                    }
                    // Our own documents save back in place; a `.chx` becomes a `.docx` beside it.
                    // Foreign files (Word, text) go through 另存为 so they are never overwritten.
                    let ours = l.seed.is_some();
                    app.file = ours.then(|| path.with_extension("docx"));
                    if ours && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("chx")) {
                        let to = path.with_extension("docx");
                        window.push_notification(Notification::info(format!("旧格式 .chx 已打开，保存时将写入 {}", to.display())), cx);
                    }
                    app.doc.update(cx, |d, cx| d.replace_doc(l.doc, cx));
                    app.close_backstage(window, cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("打开失败：{e}")), cx),
            });
        })
        .detach();
    }

    fn save_doc(&mut self, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc.read(cx).doc().clone();
        let seed = cx.global::<AppState>().settings.style.handwriting.seed;
        let tables = file_ops::table_shapes(cx.global::<AppState>().active_template().as_ref());
        if let (false, Some(path)) = (save_as, self.file.clone()) {
            let msg = match file_ops::save(&path, &doc, seed, &tables) {
                Ok(()) => Notification::success(format!("已保存 {}", path.display())),
                Err(e) => Notification::error(format!("保存失败：{e}")),
            };
            window.push_notification(msg, cx);
            self.close_backstage(window, cx);
            return;
        }
        let rx = cx.prompt_for_new_path(&file_ops::documents_dir(), Some("文档.docx"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else { return };
            if path.extension().is_none() {
                path.set_extension("docx");
            }
            let res = file_ops::save(&path, &doc, seed, &tables);
            let _ = this.update_in(cx, |app, window, cx| {
                let msg = match res {
                    Ok(()) => {
                        app.file = Some(path.clone());
                        Notification::success(format!("已保存 {}", path.display()))
                    }
                    Err(e) => Notification::error(format!("保存失败：{e}")),
                };
                window.push_notification(msg, cx);
                app.close_backstage(window, cx);
            });
        })
        .detach();
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc.read(cx).document();
        if doc.stroke_count() == 0 {
            window.push_notification(Notification::warning("没有可导出的笔画（请先选择字体并输入文字）"), cx);
            return;
        }
        let profile = cx.global::<AppState>().settings.device.profile.clone();
        let pages = self.written_pages(cx);
        let rx = cx.prompt_for_new_path(&file_ops::documents_dir(), Some("chixie.gcode"));
        cx.spawn_in(window, async move |_this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let msg = match file_ops::export_files(&pages, &profile, &path) {
                Ok(files) => Notification::success(format!("已导出 {} 个文件：{}", files.len(), files[0].display())),
                Err(e) => Notification::error(format!("导出失败：{e}")),
            };
            let _ = cx.update(|window, cx| window.push_notification(msg, cx));
        })
        .detach();
    }

    fn import_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择炽写 config.ini".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let res = std::fs::read(&path).map(|b| import_chixie3(&String::from_utf8_lossy(&b)));
            let _ = this.update_in(cx, |app, window, cx| match res {
                Ok(style) => {
                    app.edit_settings(cx, |st| st.settings.style = style);
                    window.push_notification(Notification::success("已导入炽写的手写效果设置"), cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("导入失败：{e}")), cx),
            });
        })
        .detach();
    }

    fn open_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fonts = self.fonts.clone();
        fonts.update(cx, |f, cx| f.refresh_collected(cx));
        window.open_dialog(cx, move |dialog, _w, _cx| {
            dialog.title("字体库").w(px(960.)).child(div().h(px(640.)).child(fonts.clone()))
        });
    }

    fn open_style_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_style_tab(0, window, cx);
    }

    /// 手写效果范围 on tab `tab` (0 中文, 1 英文, 2 数字).
    pub(crate) fn open_style_tab(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.style_dialog.update(cx, |d, cx| d.show_tab(tab, window, cx));
        let content = self.style_dialog.clone();
        window.open_dialog(cx, move |dialog, _w, _cx| dialog.title("手写效果范围").w(px(460.)).child(content.clone()));
    }

    fn reshuffle(&mut self, cx: &mut Context<Self>) {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        self.edit_settings(cx, |st| st.settings.style.handwriting.seed = seed);
    }

    pub fn open_backstage(&mut self, stage: Stage, window: &mut Window, cx: &mut Context<Self>) {
        if stage == Stage::Templates && self.backstage != Some(Stage::Templates) {
            let st = cx.global::<AppState>();
            let active = st.settings.page.template.clone();
            let ix = active.and_then(|n| st.templates.templates.iter().position(|t| t.name == n)).unwrap_or(0);
            self.templates.update(cx, |v, cx| v.select(ix, window, cx));
        }
        self.backstage = Some(stage);
        window.focus(&self.backstage_focus, cx);
        cx.notify();
    }

    pub fn close_backstage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.backstage = None;
        self.focus_doc(window, cx);
        cx.notify();
    }

    fn zoom_by(&mut self, f: f32, cx: &mut Context<Self>) {
        self.doc.update(cx, |d, cx| {
            let z = d.zoom() * f;
            d.set_zoom(z, cx)
        });
    }

    // ---- device ----

    /// Patrol (pen up: straight away; it leaves no mark) or draw the ruling (confirmed first).
    pub(crate) fn start_tool(&mut self, tool: PageTool, window: &mut Window, cx: &mut Context<Self>) {
        if self.device.read(cx).busy() {
            window.push_notification(Notification::warning("当前有书写任务正在进行中，请先结束任务"), cx);
            return;
        }
        let doc = self.doc.read(cx).document();
        let here = self.doc.read(cx).caret_page().min(doc.pages.len().saturating_sub(1));
        let written = self.written_pages(cx);
        let Some(page) = written.get(here).map(|w| &w.page) else { return };
        let profile = cx.global::<AppState>().settings.device.profile.clone();
        let (gcode, title) = match tool {
            PageTool::Patrol(mode, template) => {
                let area = if template { job::template_area(page) } else { job::written_area(page) };
                let Some(r) = area else {
                    let msg = if template { "该页没有模板框线，无法巡边" } else { "没有书写内容，无法巡边" };
                    window.push_notification(Notification::warning(msg), cx);
                    return;
                };
                (job::patrol(r, mode, &profile), (mode != PatrolMode::Up).then_some(format!("是否开始{}？（请确保已经设置过原点）", mode.label())))
            }
            PageTool::Ruling => {
                if page.guides.is_empty() {
                    window.push_notification(Notification::warning("该页没有模板框线可绘制"), cx);
                    return;
                }
                (job::ruling(&page.guides, &profile), Some("是否开始绘制页面框架线？".to_string()))
            }
        };
        self.show_pane = true;
        self.resume.job_started(None);
        let job = Job { pages: vec![gcode], first_page: here, ..Default::default() };
        let Some(title) = title else {
            self.device.update(cx, |d, cx| d.run(job, cx));
            return;
        };
        let device = self.device.clone();
        window.open_alert_dialog(cx, move |a, _w, _cx| {
            let (device, job) = (device.clone(), job.clone());
            a.title(title.clone()).description("笔会落在纸上。请确认纸张和原点。").confirm().ok_text("开始").on_ok(move |_, _w, cx| {
                device.update(cx, |d, cx| d.run(job.clone(), cx));
                true
            })
        });
    }

    /// The page 书写 writes: the one holding the caret (选页 moves the caret).
    pub(crate) fn write_page(&self, cx: &App) -> usize {
        let view = self.doc.read(cx);
        view.caret_page().min(view.layout_doc().pages.len().saturating_sub(1))
    }

    /// Put the caret at the start of page `k` and scroll there (选页).
    pub(crate) fn goto_page(&mut self, k: usize, cx: &mut Context<Self>) {
        let start = self.doc.read(cx).layout_doc().lines.iter().find(|l| l.page == k).map(|l| l.start);
        if let Some(i) = start {
            self.doc.update(cx, |d, cx| d.select(i..i, cx));
        }
        cx.notify();
    }

    /// 书写 (选页书写): confirm extents, then stream the selected page only. The pen parks at
    /// 写完位置 after it, and the next page is selected for after the sheet change.
    pub(crate) fn start_job(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.device.read(cx).busy() {
            window.push_notification(Notification::warning("当前有书写任务正在进行中，请先结束任务"), cx);
            return;
        }
        let first = self.write_page(cx);
        let written = self.written_pages(cx);
        let Some(chosen) = written.get(first..first + 1) else { return };
        let strokes: usize = chosen.iter().map(|w| w.page.strokes.len()).sum();
        if strokes == 0 {
            window.push_notification(Notification::warning(format!("第 {} 页没有书写内容", first + 1)), cx);
            return;
        }
        let profile = cx.global::<AppState>().settings.device.profile.clone();
        let pts = || chosen.iter().flat_map(|w| w.page.strokes.iter().flatten());
        let (w, h) = match pts().next() {
            Some(&(x, y)) => {
                let (x0, y0, x1, y1) = pts().fold((x, y, x, y), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)));
                (x1 - x0, y1 - y0)
            }
            None => (0.0, 0.0),
        };
        // The machine's own limits ($$) when known, else the verified board's.
        let machine = self.device.read(cx).params.read(cx).machine.clone();
        let (tx, ty) = machine.as_ref().and_then(|m| m.travel).unwrap_or((TRAVEL_MM, TRAVEL_MM));
        let over = w > tx || h > ty;
        let mut warn = String::new();
        if over {
            warn.push_str(&format!("⚠ 超出机器行程 {tx:.0} × {ty:.0} mm！"));
        }
        if let Some(max) = machine.as_ref().and_then(|m| m.max_feed()).filter(|m| profile.write_speed > *m) {
            warn.push_str(&format!("写字速度 {:.0} 高于机器最大速度 {max:.0}，将按最大速度运行。", profile.write_speed));
        }
        if !profile.firmware.supported() {
            window.push_notification(Notification::error("当前固件设置为 Marlin，暂不支持书写（设备 › 参数设置）"), cx);
            return;
        }
        let pages = written.len();
        let desc = format!(
            "第 {} / {pages} 页（{strokes} 笔），书写范围 {w:.1} × {h:.1} mm。{warn}请确认纸张已放好、笔已装好，起点为当前原点；写完后笔会停在{}。",
            first + 1,
            profile.end_position.label(),
        );
        let title = format!("是否立即书写第 {} 页？", first + 1);
        self.show_pane = true;
        let app = cx.entity();
        window.open_alert_dialog(cx, move |a, _w, _cx| {
            let app = app.clone();
            a.title(title.clone()).description(desc.clone()).confirm().ok_text("开始").on_ok(move |_, _w, cx| {
                app.update(cx, |t, cx| t.run_page(first, cx));
                true
            })
        });
    }

    /// Stream page `page` now (no prompt).
    pub(crate) fn run_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let written = self.written_pages(cx);
        let Some(chosen) = written.get(page..page + 1) else { return };
        let profile = cx.global::<AppState>().settings.device.profile.clone();
        let codes: Vec<gcode::PageCode> = chosen.iter().map(|w| gcode::emit_written(w, &profile)).collect();
        let ids = self.doc.read(cx).doc().ids().to_vec();
        self.resume.job_started(Some(resume::JobMeta::new(page, &codes, chosen, &ids)));
        self.writing_started(page);
        let job = job_of(page, codes);
        self.device.update(cx, |d, cx| d.run(job, cx));
    }

    /// A 书写 job on `page` starts: the 换纸 card is done with.
    pub(crate) fn writing_started(&mut self, page: usize) {
        (self.writing, self.sheet_change) = (Some(page), None);
    }

    /// A 书写 job ended: done → select the next page and ask for a new sheet (a toast and the
    /// task pane card; nothing blocks jogging or 设为原点 meanwhile). Returns the toast text.
    fn writing_ended(&mut self, ev: &DevEvent, cx: &mut Context<Self>) -> Option<String> {
        let DevEvent::JobEnded { outcome, .. } = ev else { return None };
        let page = self.writing.take()?;
        if *outcome != JobOutcome::Done {
            return None;
        }
        let pages = self.doc.read(cx).layout_doc().pages.len();
        let park = cx.global::<AppState>().settings.device.profile.end_position.label();
        if page + 1 >= pages {
            return Some(format!("第 {} 页已写完（全部 {pages} 页），笔已停在{park}", page + 1));
        }
        self.sheet_change = Some((page, page + 1));
        self.goto_page(page + 1, cx);
        Some(format!("第 {} 页已写完，笔已停在{park}。请换纸，然后点「书写」写第 {} 页（已选中）", page + 1, page + 2))
    }

    fn on_device_event(&mut self, ev: DevEvent, window: &mut Window, cx: &mut Context<Self>) {
        let at = {
            let doc = self.doc.read(cx).doc();
            self.resume.observe(&ev, doc.chars(), doc.ids())
        };
        let interrupted = matches!(&ev, DevEvent::JobEnded { outcome: JobOutcome::Stopped | JobOutcome::Failed(_), .. });
        let page_done = self.writing_ended(&ev, cx);
        let notice = self.device.update(cx, |d, cx| d.on_event(ev, cx));
        match notice {
            Notice::None => {}
            Notice::Info(m) => window.push_notification(Notification::info(page_done.unwrap_or(m)), cx),
            Notice::Warn(m) => {
                let m = match at {
                    Some(c) if interrupted => format!("{m}。如需续写，可用「断点续写」（已记住「{c}」）"),
                    Some(c) => format!("{m}（将从「{c}」所在笔画继续）"),
                    None => m,
                };
                window.push_notification(Notification::warning(m), cx)
            }
            Notice::Error(m) => {
                let m = match at {
                    Some(c) if interrupted => format!("{m}。如需续写，可用「断点续写」（已记住「{c}」）"),
                    _ => m,
                };
                window.push_notification(Notification::error(m), cx)
            }
            Notice::Alarm(m) => {
                let device = self.device.clone();
                let n = Notification::warning(m).action(move |_, _, _| {
                    let device = device.clone();
                    Button::new("alarm-unlock").small().primary().label("解锁").on_click(move |_, _w, cx| {
                        device.read(cx).send(DevCommand::Unlock);
                    })
                });
                window.push_notification(n, cx);
            }
            Notice::AwaitingPen(colour) => {
                let device = self.device.clone();
                let name = chixie_rs_core::profile::colour_name(colour);
                window.open_alert_dialog(cx, move |a, _w, _cx| {
                    let (ok, cancel) = (device.clone(), device.clone());
                    a.title("换笔提示")
                        .description(format!("您已启用多色绘制，请更换笔颜色为：{name}"))
                        .confirm()
                        .ok_text("换笔完成")
                        .cancel_text("停止任务")
                        .on_ok(move |_, _w, cx| {
                            ok.update(cx, |d, cx| d.next_sheet(cx));
                            true
                        })
                        .on_cancel(move |_, _w, cx| {
                            cancel.update(cx, |d, cx| d.stop(cx));
                            true
                        })
                });
            }
        }
        cx.notify();
    }

    // ---- chrome ----

    /// Quick access toolbar + 文件 + tab titles + collapse chevron.
    /// Document name shown in the title bar and the taskbar (Word: "文档1 - Word").
    fn doc_title(&self) -> String {
        let name = self.file.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "文档1".into());
        format!("{name} - chixie-rs")
    }

    /// Word's caption: app mark + quick access toolbar on the left, the document name centred,
    /// the account and the window controls on the right. The whole bar drags the window.
    #[inline(never)]
    fn title_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = ribbon::chrome(cx);
        let st = cx.global::<AppState>();
        let user = st.user.as_ref().map(|u| u.name.clone());
        let act = |a: Box<dyn Action>| ribbon_tabs::editor_action(self.doc.clone(), a);
        let in_backstage = self.backstage.is_some();
        let qat = h_flex()
            .gap_0p5()
            // Word for Mac shows no app mark next to the traffic lights.
            .when(!cfg!(target_os = "macos"), |el| el.child(
                div()
                    .size(px(20.))
                    .mr_2()
                    .rounded_sm()
                    .bg(c.title_fg)
                    .text_color(c.title_bg)
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("写"),
            ))
            .when(!in_backstage, |el| {
                el.child(
                    ribbon::title_btn("qat-save", IconName::Save, format!("保存 ({})", ribbon::shortcut("S")), &c)
                        .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.save_doc(false, w, cx))),
                )
                .child(ribbon::title_btn("qat-undo", IconName::Undo2, format!("撤销 ({})", ribbon::shortcut("Z")), &c).on_click(act(Box::new(doc_view::Undo))))
                .child(ribbon::title_btn("qat-redo", IconName::Redo2, format!("重做 ({})", ribbon::redo_shortcut()), &c).on_click(act(Box::new(doc_view::Redo))))
            });
        let account = match user {
            Some(name) => h_flex()
                .gap_2()
                .px_2()
                .child(div().text_xs().child(name.clone()))
                .child(
                    div()
                        .size(px(24.))
                        .rounded_full()
                        .bg(c.title_hover)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .child(name.chars().next().map(String::from).unwrap_or_default()),
                )
                .into_any_element(),
            None => div()
                .id("login-btn")
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .h(px(26.))
                .px_3()
                .rounded_sm()
                .flex()
                .items_center()
                .text_xs()
                .cursor_pointer()
                .hover({
                    let h = c.title_hover;
                    move |s| s.bg(h)
                })
                .tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("登录后可下载云端字体").build(w, cx))
                .child("登录")
                .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::Account, w, cx)))
                .into_any_element(),
        };
        let content = h_flex()
            .id("title-content")
            .relative()
            .flex_1()
            .h_full()
            .pl_2()
            // Centred like Word, independent of the side widths.
            .child(div().absolute().inset_0().flex().items_center().justify_center().text_sm().child(self.doc_title()))
            .child(qat)
            .child(div().flex_1())
            .child(account);
        if cfg!(target_os = "windows") {
            // Our own buttons; the OS handles drag, snap and the buttons through hit-test areas.
            h_flex()
                .id("title-bar")
                .w_full()
                .h(px(ribbon::TITLE_H))
                .flex_shrink_0()
                .bg(c.title_bg)
                .text_color(c.title_fg)
                .child(content.window_control_area(WindowControlArea::Drag))
                .child(ribbon::window_controls(window, &c))
                .into_any_element()
        } else {
            // macOS: room for the system traffic lights on the left. Linux: gpui-component draws
            // the window buttons (only under client-side decorations). Both drag by mouse-move.
            TitleBar::new()
                .h(px(ribbon::TITLE_H))
                .bg(c.title_bg)
                .text_color(c.title_fg)
                .border_color(cx.theme().border.opacity(0.6))
                .child(content)
                .into_any_element()
        }
    }

    /// 文件 + tab titles + collapse chevron, on the light strip above the ribbon card.
    #[inline(never)]
    fn tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .h(px(34.))
            .px_2()
            .gap_0p5()
            .items_end()
            .child(ribbon::tab_item(TABS.len(), "文件", false, cx).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_backstage(Stage::New, w, cx))))
            .children(TABS.iter().enumerate().map(|(i, label)| {
                ribbon::tab_item(i, label, i == self.tab, cx).on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                    t.tab = i;
                    t.ribbon_collapsed = false;
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .child(
                ribbon::icon_btn(
                    "collapse",
                    if self.ribbon_collapsed { IconName::ChevronDown } else { IconName::ChevronUp },
                    if self.ribbon_collapsed { "展开功能区" } else { "折叠功能区" },
                )
                .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                    t.ribbon_collapsed = !t.ribbon_collapsed;
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let font = st.active_font.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| "未选择字体".into());
        let template = st.settings.page.template.clone().unwrap_or_else(|| "空白页".into());
        let d = self.doc.read(cx);
        let layout = d.document();
        let pages = layout.pages.len().max(1);
        let words = d.doc().chars().iter().filter(|c| !c.is_whitespace()).count();
        let missing = layout.missing.len();
        let punct_issues = layout.punct_issues.len();
        let zoom_pct = (d.zoom() / doc_view::ZOOM_100 * 100.0).round();
        let caret_page = d.caret_page() + 1;
        let dev = self.device.read(cx);
        let (dev_summary, dev_connected, job) = (dev.summary(), dev.connected.is_some(), dev.job);
        let theme = cx.theme();
        let (muted, hover, primary, danger) = (theme.muted_foreground, theme.secondary_hover, theme.primary, theme.danger);
        let strip = ribbon::chrome(cx).strip_bg;
        // Word's status bar items are flat cells that highlight on hover.
        let cell = move |id: &'static str| {
            h_flex().id(id).h_full().px_2().gap_1().items_center().hover(move |s| s.bg(hover))
        };
        h_flex()
            .w_full()
            .h(px(24.))
            .px_1()
            .flex_shrink_0()
            .bg(strip)
            .text_color(muted)
            .text_xs()
            .child(cell("sb-page").child(format!("第 {caret_page} 页，共 {pages} 页")))
            .child(cell("sb-words").child(format!("{words} 个字")))
            .when(missing > 0, |el| {
                let chars: String = layout.missing.iter().take(12).collect();
                el.child(
                    cell("sb-missing")
                        .cursor_pointer()
                        .text_color(danger)
                        .child(gpui_kit::component::Icon::new(IconName::TriangleAlert).with_size(px(12.)))
                        .child(format!("缺字 {missing}：{chars}"))
                        .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_nav(crate::nav_pane::NavTab::Missing, w, cx))),
                )
            })
            .when(punct_issues > 0, |el| {
                // 位置合理性检测: lines that still start with a closing mark (避头尾 off, or no
                // way to move it). Each click jumps to the next one.
                let doc = self.doc.clone();
                el.child(
                    cell("sb-punct")
                        .cursor_pointer()
                        .text_color(danger)
                        .child(gpui_kit::component::Icon::new(IconName::TriangleAlert).with_size(px(12.)))
                        .child(format!("存在 {punct_issues} 处标点位置异常"))
                        .on_click(ribbon_tabs::editor_action(doc, Box::new(doc_view::NextPunctIssue))),
                )
            })
            .child(
                cell("sb-font")
                    .cursor_pointer()
                    .child(format!("字体：{font}"))
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.open_fonts(w, cx))),
            )
            .child(cell("sb-template").child(format!("模板：{template}")))
            .child(
                cell("sb-device")
                    .cursor_pointer()
                    .child(div().size(px(7.)).rounded_full().bg(if dev_connected { gpui_kit::green() } else { muted.opacity(0.6) }))
                    .child(dev_summary)
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.tab = TAB_DEVICE;
                        t.show_pane = true;
                        t.ribbon_collapsed = false;
                        cx.notify();
                    })),
            )
            .when_some(self.sheet_change.filter(|_| job.is_none()), |el, (_, next)| el.child(cell("sb-sheet").text_color(primary).child(format!("请换纸 · 下一页：第 {} 页", next + 1))))
            .when_some(job, |el, j| {
                let s = format!("书写 第 {} 页 {:.0}%{}", j.page + 1, j.percent(), if j.paused { "（暂停）" } else { "" });
                el.child(
                    cell("sb-job")
                        .text_color(primary)
                        .child(div().w(px(60.)).h(px(4.)).rounded_full().bg(muted.opacity(0.3)).child(
                            div().h_full().rounded_full().bg(primary).w(relative((j.percent() / 100.0).clamp(0.0, 1.0))),
                        ))
                        .child(s),
                )
            })
            .child(div().flex_1())
            .child({
                let double = cx.global::<AppState>().settings.ui.double_page;
                h_flex()
                    .child(ribbon::icon_btn("sb-single", IconName::File, "单页").selected(!double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = false)
                    })))
                    .child(ribbon::icon_btn("sb-double", IconName::Columns2, "双页").selected(double).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.edit_settings(cx, |st| st.settings.ui.double_page = true)
                    })))
            })
            .child(ribbon::icon_btn("sb-zout", IconName::Minus, "缩小").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.zoom_by(1.0 / 1.1, cx))))
            .child(div().w(px(110.)).px_1().child(Slider::new(&self.zoom_slider)))
            .child(ribbon::icon_btn("sb-zin", IconName::Plus, "放大").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.zoom_by(1.1, cx))))
            .child(
                cell("sb-zoom")
                    .w(px(52.))
                    .justify_center()
                    .cursor_pointer()
                    .child(format!("{zoom_pct}%"))
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.doc.update(cx, |d, cx| d.set_zoom(doc_view::ZOOM_100, cx));
                    })),
            )
            .into_any_element()
    }

    /// Word-style task pane: titled header with a close button over the device panel.
    #[inline(never)]
    fn task_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let border = theme.border;
        let card = ribbon::chrome(cx).card_bg;
        v_flex()
            .w(px(320.))
            .h_full()
            .flex_shrink_0()
            .bg(card)
            .border_l_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(40.))
                    .px_3()
                    .flex_shrink_0()
                    .child(div().flex_1().text_base().font_weight(FontWeight::SEMIBOLD).child("设备"))
                    .child(ribbon::icon_btn("pane-close", IconName::Close, "关闭").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.show_pane = false;
                        cx.notify();
                    }))),
            )
            .when(self.sheet_change.is_some() && !self.device.read(cx).busy(), |el| el.child(self.sheet_card(cx)))
            .when(self.resume.open, |el| el.child(self.resume_card(cx)))
            .when(self.tune_open, |el| el.child(self.tune_card(cx)))
            .child(div().flex_1().min_h_0().child(self.device.clone()))
            .into_any_element()
    }

    /// 换纸 card: shown after a page is written, until the next 书写. Information only: jogging,
    /// 设为原点 and every other control stay usable.
    fn sheet_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let Some((done, next)) = self.sheet_change else { return div().into_any_element() };
        v_flex()
            .gap_1()
            .p_3()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("请换纸"))
                    .child(Button::new("sheet-close").ghost().xsmall().label("关闭").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.sheet_change = None;
                        cx.notify();
                    }))),
            )
            .child(div().text_sm().child(format!("第 {} 页已写完。下一页：第 {} 页（已选中）", done + 1, next + 1)))
            .child(div().text_xs().text_color(muted).child("放好新纸；需要时点动微调并点「设为原点」。准备好后在「设备 › 书写」点「书写」。"))
            .into_any_element()
    }

    /// Keep the status bar slider on the editor's zoom (Ctrl+wheel, buttons, 页宽 change it too).
    fn sync_zoom_slider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pct = self.doc.read(cx).zoom() / doc_view::ZOOM_100 * 100.0;
        let v = zoom_to_slider(pct);
        if (self.zoom_slider.read(cx).value().start() - v).abs() > 0.01 {
            self.zoom_slider.update(cx, |s, cx| s.set_value(v, window, cx));
        }
    }
}

/// Word's zoom slider is two-sided: the left half spans the minimum..100 %, the right half
/// 100 %..maximum, so 100 % always sits in the middle.
fn zoom_to_slider(pct: f32) -> f32 {
    let (lo, hi) = zoom_pct_range();
    if pct <= 100.0 { (pct - lo) / (100.0 - lo) * 50.0 } else { 50.0 + (pct - 100.0) / (hi - 100.0) * 50.0 }.clamp(0.0, 100.0)
}

fn slider_to_zoom(v: f32) -> f32 {
    let (lo, hi) = zoom_pct_range();
    if v <= 50.0 { lo + v / 50.0 * (100.0 - lo) } else { 100.0 + (v - 50.0) / 50.0 * (hi - 100.0) }
}

fn zoom_pct_range() -> (f32, f32) {
    (doc_view::ZOOM_MIN / doc_view::ZOOM_100 * 100.0, doc_view::ZOOM_MAX / doc_view::ZOOM_100 * 100.0)
}

impl Render for ChixieRsApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = (theme.background, theme.foreground, theme.border);
        let c = ribbon::chrome(cx);
        let title = self.doc_title();
        if title != self.win_title {
            window.set_window_title(&title);
            self.win_title = title;
        }
        let title_bar = self.title_bar(window, cx);
        let body = if let Some(stage) = self.backstage {
            self.render_backstage(stage, cx)
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    v_flex()
                        .w_full()
                        .flex_shrink_0()
                        .bg(c.strip_bg)
                        .child(self.tab_strip(cx))
                        .when(!self.ribbon_collapsed, |el| {
                            el.child(
                                div().w_full().px_2().pb_1p5().child(
                                    div()
                                        .w_full()
                                        .h(px(RIBBON_H))
                                        .py_1()
                                        .rounded_lg()
                                        .bg(c.card_bg)
                                        .border_1()
                                        .border_color(border.opacity(0.7))
                                        .shadow_xs()
                                        .overflow_hidden()
                                        .child(self.ribbon_body(cx)),
                                ),
                            )
                        })
                        .when(self.ribbon_collapsed, |el| el.pb_1()),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .border_t_1()
                        .border_color(border.opacity(0.6))
                        .when(self.nav_open, |el| el.child(self.nav.clone()))
                        .child(
                            v_flex()
                                .flex_1()
                                .h_full()
                                .min_w_0()
                                .when(self.find_open, |el| el.child(self.find_bar.clone()))
                                .child(div().flex_1().min_h_0().w_full().child(self.doc.clone())),
                        )
                        .when(self.show_pane, |el| el.child(self.task_pane(cx))),
                )
                .child(self.status_bar(cx))
                .into_any_element()
        };
        let root = v_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .on_action(cx.listener(|t, _: &doc_view::Save, w, cx| t.save_doc(false, w, cx)))
            .map(|el| self.menu_actions(el, cx))
            .on_action(cx.listener(|t, _: &crate::find_bar::OpenFind, w, cx| t.open_nav(crate::nav_pane::NavTab::Search, w, cx)))
            .on_action(cx.listener(|t, _: &crate::find_bar::OpenReplace, w, cx| t.open_find(true, w, cx)))
            .child(title_bar)
            .child(body);
        if cfg!(target_os = "linux") {
            // Client-side decorations: shadow and resize edges (a no-op under a server frame).
            gpui_kit::component::window_border().child(root).into_any_element()
        } else {
            root.into_any_element()
        }
    }
}

/// Name for a font whose real name is not known yet (fixed by `repair_font_names`).
fn placeholder_name(id: i64) -> String {
    format!("字体 #{id}")
}

fn is_placeholder_name(name: &str) -> bool {
    name.starts_with("字体 #")
}

pub(crate) fn fmt_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn parse_in(s: &Entity<InputState>, cx: &App, min: f32, max: f32) -> Option<f32> {
    s.read(cx).value().trim().parse::<f32>().ok().filter(|v| (min..=max).contains(v))
}
