//! 字体库: fonts saved on this computer (usable offline, no login), the account's collected fonts
//! and the online catalog (1-based pages of 9, with a name filter; both need login).
//! Every tab is a 3×3 grid of cards with a large preview — pages of nine, never a scrollbar.
//! Downloaded fonts preview in their own strokes; the rest use the server's preview image
//! (`cloud_preview`), fetched in the background with a skeleton while it loads.
//! Cloud "使用" fetches and decrypts the font in the session worker; the app saves it to the local
//! store and makes it active when `FontLoaded` arrives. Local "使用" is handled by the app.

use std::sync::Arc;

use chixie_rs_core::api::FontInfo;
use chixie_rs_core::session::{Command, FontListKind};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::fonts_ctl::Role;
use chixie_rs_core::font::library::FontKey;
use crate::cloud_preview::{self, CloudImages, CloudPreview};
use crate::state::AppState;

/// Cards per page (the server's catalog page size).
const PER_PAGE: usize = 9;
const COLS: usize = 3;

pub enum FontsEvent {
    /// Use a font from the local store.
    UseLocal(i64),
    /// Delete a font from the local store.
    RemoveLocal(i64),
    /// The user wants to log in (cloud tabs while logged out).
    Login,
    /// 设为主线 / 英文 / 符号 / 数字字体.
    SetRole(i64, Role),
    /// Stop using a role font (falls back to the main font).
    ClearRole(Role),
    /// 加入混用 / 移出混用.
    ToggleMix(i64),
    /// New weight of a font in the mix.
    MixWeight(i64, f32),
    /// 换一组.
    RerollMix,
    /// 在文件夹中显示.
    Reveal(i64),
    /// 导出….
    Export(i64),
    /// 导入….
    Import,
    /// Open the default library folder.
    OpenFolder,
}

pub struct FontsView {
    /// 0 本地字体, 1 我的收藏, 2 在线字体库.
    tab: usize,
    collected: Vec<FontInfo>,
    online: Vec<FontInfo>,
    /// Online catalog page (1-based, server side).
    page: i64,
    /// Client-side pages (0-based) of the local and collected lists.
    local_page: usize,
    collected_page: usize,
    online_requested: bool,
    search: Entity<InputState>,
    loading_font: Option<i64>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<FontsEvent> for FontsView {}

/// What a card shows, from either a local font or a server listing.
struct Card {
    id: i64,
    name: SharedString,
    glyphs: i64,
    tags: Vec<String>,
    collections: Option<i64>,
    description: Option<String>,
    saved: bool,
    local: bool,
}

impl FontsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索字体名称"));
        let sub = cx.subscribe_in(&search, window, |this, _s, ev: &InputEvent, _w, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.request_online(1, cx);
            }
        });
        let obs = cx.observe_global::<AppState>(|_, cx| cx.notify());
        Self {
            tab: 0,
            collected: Vec::new(),
            online: Vec::new(),
            page: 1,
            local_page: 0,
            collected_page: 0,
            online_requested: false,
            search,
            loading_font: None,
            _subs: vec![sub, obs],
        }
    }

    pub fn set_list(&mut self, kind: FontListKind, page: i64, fonts: Vec<FontInfo>, cx: &mut Context<Self>) {
        self.fetch_previews(&fonts, cx);
        match kind {
            FontListKind::Collected => {
                self.collected = fonts;
                self.collected_page = self.collected_page.min(pages(self.collected.len()) - 1);
            }
            FontListKind::Online => {
                self.page = page;
                self.online = fonts;
            }
        }
        cx.notify();
    }

    /// Start background fetches of server preview images not seen yet.
    fn fetch_previews(&mut self, fonts: &[FontInfo], cx: &mut Context<Self>) {
        let dir = cx.global::<AppState>().fonts.cache_dir().to_path_buf();
        let todo: Vec<(i64, Option<String>)> = {
            let st = cx.global::<AppState>();
            fonts.iter().filter(|f| !st.cloud_previews.contains_key(&f.id)).map(|f| (f.id, f.font_preview.clone())).collect()
        };
        for (id, url) in todo {
            let Some(url) = url.filter(|u| u.starts_with("http")) else {
                cx.global_mut::<AppState>().cloud_previews.insert(id, CloudPreview::Missing);
                continue;
            };
            cx.global_mut::<AppState>().cloud_previews.insert(id, CloudPreview::Loading);
            let dir = dir.clone();
            let task = cx.background_executor().spawn(async move { cloud_preview::load_or_fetch(&dir, id, &url).map(|m| CloudImages::new(&m)) });
            cx.spawn(async move |_this, cx| {
                let result = task.await;
                cx.update(|cx| {
                    let p = match result {
                        Ok(im) => CloudPreview::Ready(Arc::new(im)),
                        Err(e) => {
                            log::warn!("preview of font {id}: {e}");
                            CloudPreview::Missing
                        }
                    };
                    cx.global_mut::<AppState>().cloud_previews.insert(id, p);
                });
            })
            .detach();
        }
    }

    pub fn collected(&self) -> &[FontInfo] {
        &self.collected
    }

    /// Forget cloud lists (after logout).
    pub fn clear_cloud(&mut self, cx: &mut Context<Self>) {
        self.collected.clear();
        self.online.clear();
        self.online_requested = false;
        cx.notify();
    }

    pub fn name_of(&self, id: i64) -> Option<String> {
        self.collected.iter().chain(&self.online).find(|f| f.id == id).map(|f| f.font_name.clone())
    }

    pub fn font_loaded(&mut self, cx: &mut Context<Self>) {
        self.loading_font = None;
        cx.notify();
    }

    pub fn refresh_collected(&mut self, cx: &mut Context<Self>) {
        if cx.global::<AppState>().user.is_some() {
            cx.global::<AppState>().session.send(Command::CollectedFonts);
        }
    }

    fn request_online(&mut self, page: i64, cx: &mut Context<Self>) {
        self.online_requested = true;
        let search = self.search.read(cx).value().trim().to_string();
        cx.global::<AppState>().session.send(Command::OnlineFonts { page: page.max(1), search });
    }

    /// Download (login needed) and use a cloud font.
    pub fn use_font(&mut self, id: i64, cx: &mut Context<Self>) {
        self.loading_font = Some(id);
        cx.global::<AppState>().session.send(Command::LoadFont { id });
        cx.notify();
    }

    /// The cards of the current tab and page.
    fn cards(&self, cx: &App) -> Vec<Card> {
        let st = cx.global::<AppState>();
        let local = st.fonts.list();
        let cloud = |f: &FontInfo| Card {
            id: f.id,
            name: f.font_name.clone().into(),
            glyphs: f.character_number,
            tags: f.tag_list(),
            collections: f.collection_count,
            description: f.font_description.clone().filter(|d| !d.trim().is_empty()),
            saved: local.iter().any(|l| l.id == f.id),
            local: false,
        };
        match self.tab {
            0 => local
                .iter()
                .skip(self.local_page * PER_PAGE)
                .take(PER_PAGE)
                .map(|l| {
                    // Server details when the font is also in a loaded list.
                    let info = self.collected.iter().chain(&self.online).find(|f| f.id == l.id);
                    Card {
                        id: l.id,
                        name: l.name.clone().into(),
                        glyphs: l.glyphs as i64,
                        tags: {
                            use chixie_rs_core::font::svg::FontSource;
                            let mut t: Vec<String> = match l.source {
                                FontSource::Imported => vec!["导入".into()],
                                FontSource::Generated => vec!["自制".into()],
                                FontSource::Cloud { .. } => Vec::new(),
                            };
                            if l.copies > 0 {
                                t.push(format!("另有 {} 份", l.copies));
                            }
                            t.extend(info.map(FontInfo::tag_list).unwrap_or_default());
                            t
                        },
                        collections: info.and_then(|f| f.collection_count),
                        description: info.and_then(|f| f.font_description.clone()),
                        saved: true,
                        local: true,
                    }
                })
                .collect(),
            1 => self.collected.iter().skip(self.collected_page * PER_PAGE).take(PER_PAGE).map(cloud).collect(),
            _ => self.online.iter().map(cloud).collect(),
        }
    }

    fn card(&self, c: Card, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let is_active = st.active_font.as_ref().map(|f| f.id) == Some(c.id);
        let loading = self.loading_font == Some(c.id);
        let busy = self.loading_font.is_some();
        let theme = cx.theme();
        let (fg, muted, border, accent, hover_bg, band) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.primary,
            theme.secondary,
            if theme.mode.is_dark() { hsla(0., 0., 1., 0.03) } else { hsla(0., 0., 0., 0.025) },
        );
        let id = c.id;

        let face = crate::font_preview::face(id, 56.0, 250.0, cx).unwrap_or_else(|| match st.cloud_previews.get(&id) {
            Some(CloudPreview::Loading) => v_flex()
                .gap_2()
                .items_center()
                .child(Skeleton::new().w(px(180.)).h(px(34.)).rounded_md())
                .child(Skeleton::new().w(px(110.)).h(px(10.)).rounded_sm())
                .into_any_element(),
            // No image: the name, large and quiet, so the card still reads as a font.
            _ => v_flex()
                .items_center()
                .gap_1()
                .child(div().text_2xl().text_color(muted.opacity(0.8)).child(c.name.clone()))
                .child(div().text_xs().text_color(muted.opacity(0.7)).child("暂无预览"))
                .into_any_element(),
        });

        let action = if is_active {
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(accent)
                .child(Icon::new(IconName::Check).with_size(px(13.)))
                .child("使用中")
                .into_any_element()
        } else {
            let saved = c.saved;
            Button::new(("card-use", id as usize))
                .xsmall()
                .primary()
                .label(if saved { "使用" } else { "下载并使用" })
                .loading(loading)
                .disabled(busy)
                .on_click(cx.listener(move |this, _ev: &ClickEvent, _w, cx| {
                    if saved {
                        cx.emit(FontsEvent::UseLocal(id));
                    } else {
                        this.use_font(id, cx);
                    }
                }))
                .into_any_element()
        };

        let meta = {
            let mut parts: Vec<String> = c.tags.iter().take(3).cloned().collect();
            parts.push(format!("{} 字", c.glyphs));
            parts.join(" · ")
        };

        let description = c.description.clone();
        let badge = if c.local { role_badge(id, cx) } else { None };
        v_flex()
            .id(("font-card", id as usize))
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded_lg()
            .border_1()
            .border_color(if is_active { accent } else { border })
            .when(is_active, |el| el.shadow_xs())
            .overflow_hidden()
            .hover(move |s| s.border_color(accent.opacity(0.7)).bg(hover_bg))
            .when_some(description, |el, d| el.tooltip(move |w, cx| Tooltip::new(d.clone()).build(w, cx)))
            .child(
                // Preview band: the font's own face, centred.
                div().flex_1().min_h_0().bg(band).flex().items_center().justify_center().px_3().text_color(fg).child(face),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().min_w_0().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(c.name.clone()))
                            .when(c.local, |el| el.child(self.card_menu(id, is_active, cx)))
                            .child(action),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(muted)
                            .child(div().flex_1().min_w_0().truncate().child(meta))
                            .when(c.saved && !c.local, |el| el.child("已下载"))
                            .when_some(badge, |el, b| el.child(div().text_color(accent).child(b)))
                            .when_some(c.collections, |el, n| {
                                el.child(h_flex().gap_0p5().child(Icon::new(IconName::Star).with_size(px(11.))).child(n.to_string()))
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Three rows of three; empty slots keep their place so cards never change size.
    fn grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut cards: Vec<Option<AnyElement>> = self.cards(cx).into_iter().map(|c| Some(self.card(c, cx))).collect();
        cards.resize_with(PER_PAGE, || None);
        let mut rows = Vec::new();
        let mut it = cards.into_iter();
        for r in 0..PER_PAGE / COLS {
            let row: Vec<AnyElement> = (&mut it).take(COLS).map(|c| c.unwrap_or_else(|| div().flex_1().into_any_element())).collect();
            rows.push(h_flex().id(("font-row", r)).flex_1().min_h_0().gap_3().children(row));
        }
        v_flex().flex_1().min_h_0().gap_3().children(rows).into_any_element()
    }

    /// ‹ 第 n / m 页 › for the client-side paged tabs, ‹ 第 n 页 › for the online catalog.
    fn pager(&self, cx: &mut Context<Self>) -> AnyElement {
        let (label, prev, next) = match self.tab {
            0 => {
                let n = pages(cx.global::<AppState>().fonts.list().len());
                (format!("第 {} / {n} 页", self.local_page + 1), self.local_page > 0, self.local_page + 1 < n)
            }
            1 => {
                let n = pages(self.collected.len());
                (format!("第 {} / {n} 页", self.collected_page + 1), self.collected_page > 0, self.collected_page + 1 < n)
            }
            _ => (format!("第 {} 页", self.page), self.page > 1, self.online.len() >= PER_PAGE),
        };
        let step = |this: &mut Self, d: i64, cx: &mut Context<Self>| match this.tab {
            0 => this.local_page = (this.local_page as i64 + d).max(0) as usize,
            1 => this.collected_page = (this.collected_page as i64 + d).max(0) as usize,
            _ => this.request_online(this.page + d, cx),
        };
        h_flex()
            .gap_1()
            .child(Button::new("prev").ghost().small().icon(IconName::ChevronLeft).tooltip("上一页").disabled(!prev).on_click(cx.listener(
                move |this, _e: &ClickEvent, _w, cx| {
                    step(this, -1, cx);
                    cx.notify();
                },
            )))
            .child(div().min_w(px(84.)).flex().justify_center().text_sm().child(label))
            .child(Button::new("next").ghost().small().icon(IconName::ChevronRight).tooltip("下一页").disabled(!next).on_click(cx.listener(
                move |this, _e: &ClickEvent, _w, cx| {
                    step(this, 1, cx);
                    cx.notify();
                },
            )))
            .into_any_element()
    }
}

/// "主线 · 英文 · 混用×2" for the roles a library font plays; `None` when it plays none.
fn role_badge(id: i64, cx: &App) -> Option<String> {
    let f = &cx.global::<AppState>().settings.fonts;
    let key = FontKey::from_local_id(id);
    let mut parts: Vec<String> = Role::ALL.iter().filter(|r| r.get(&f.roles) == Some(&key)).map(|r| r.label().trim_end_matches("字体").to_string()).collect();
    if let Some(m) = f.mix.iter().find(|m| m.key == key) {
        parts.push(format!("混用×{}", m.weight));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

impl FontsView {
    /// ⋯ on a library card: roles, mix, folder, export, delete.
    fn card_menu(&self, id: i64, is_active: bool, cx: &mut Context<Self>) -> AnyElement {
        let v = cx.entity().downgrade();
        let f = cx.global::<AppState>().settings.fonts.clone();
        let key = FontKey::from_local_id(id);
        let in_mix = f.mix.iter().any(|m| m.key == key);
        Button::new(("card-menu", id as usize))
            .ghost()
            .xsmall()
            .icon(Icon::new(IconName::Ellipsis).with_size(px(13.)))
            .tooltip("更多")
            .dropdown_menu(move |m, _w, _cx| {
                let emit = |ev: fn(i64) -> FontsEvent| {
                    let v = v.clone();
                    move |_: &ClickEvent, _w: &mut Window, cx: &mut App| {
                        let _ = v.update(cx, |_, cx| cx.emit(ev(id)));
                    }
                };
                let mut m = m;
                for role in Role::ALL {
                    let v = v.clone();
                    let on = role.get(&f.roles) == Some(&key);
                    m = m.item(PopupMenuItem::new(format!("设为{}", role.label())).checked(on).on_click(move |_, _w, cx| {
                        let _ = v.update(cx, |_, cx| cx.emit(if on { FontsEvent::ClearRole(role) } else { FontsEvent::SetRole(id, role) }));
                    }));
                }
                m.separator()
                    .item(PopupMenuItem::new(if in_mix { "移出混用" } else { "加入混用" }).on_click(emit(FontsEvent::ToggleMix)))
                    .item(PopupMenuItem::new("在文件夹中显示").on_click(emit(FontsEvent::Reveal)))
                    .item(PopupMenuItem::new("导出…").on_click(emit(FontsEvent::Export)))
                    .separator()
                    .item(PopupMenuItem::new("从字体库删除").disabled(is_active).on_click(emit(FontsEvent::RemoveLocal)))
            })
            .into_any_element()
    }

    /// 本地字体 header: the role fonts, the mix (weights, 换一组), import and the folder.
    fn library_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppState>();
        let f = st.settings.fonts.clone();
        let name = |k: Option<&FontKey>| k.and_then(|k| st.fonts.name_of(k.local_id())).unwrap_or_else(|| "—".into());
        let muted = cx.theme().muted_foreground;
        let roles = Role::ALL
            .iter()
            .map(|r| {
                let who = if *r != Role::Main && r.get(&f.roles).is_none() { "同主线".to_string() } else { name(r.get(&f.roles)) };
                format!("{}：{who}", r.label().trim_end_matches("字体"))
            })
            .collect::<Vec<_>>()
            .join("　");
        let mix: Vec<(i64, String, f32)> = f.mix.iter().map(|m| (m.key.local_id(), name(Some(&m.key)), m.weight)).collect();
        let v = cx.entity().downgrade();
        let mut mix_row = h_flex().gap_1().text_xs().text_color(muted).child("混用：");
        if mix.is_empty() {
            mix_row = mix_row.child("未启用（卡片 ⋯ › 加入混用）");
        }
        for (id, n, w) in mix {
            let v = v.clone();
            mix_row = mix_row.child(Button::new(("mixw", id as usize)).ghost().xsmall().label(format!("{n} ×{w}")).dropdown_menu(move |m, _w, _cx| {
                [1.0f32, 2.0, 3.0, 5.0].into_iter().fold(m, |m, x| {
                    let v = v.clone();
                    m.item(PopupMenuItem::new(format!("权重 {x}")).checked(x == w).on_click(move |_, _w, cx| {
                        let _ = v.update(cx, |_, cx| cx.emit(FontsEvent::MixWeight(id, x)));
                    }))
                })
            }));
        }
        let mix_on = !f.mix.is_empty();
        v_flex()
            .gap_1()
            .child(h_flex().gap_2().text_xs().child(div().flex_1().min_w_0().truncate().child(roles)).child(div().text_color(muted).child("（卡片 ⋯ 可设置）")))
            .child(
                h_flex()
                    .gap_2()
                    .child(mix_row)
                    .when(mix_on, |el| {
                        el.child(Button::new("reroll").ghost().xsmall().icon(IconName::Shuffle).label("换一组").on_click(cx.listener(|_t, _e: &ClickEvent, _w, cx| cx.emit(FontsEvent::RerollMix))))
                    })
                    .child(div().flex_1())
                    .child(Button::new("import").ghost().xsmall().icon(IconName::FolderInput).label("导入…").on_click(cx.listener(|_t, _e: &ClickEvent, _w, cx| cx.emit(FontsEvent::Import))))
                    .child(
                        Button::new("open-dir")
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderOpen)
                            .label("打开字体文件夹")
                            .on_click(cx.listener(|_t, _e: &ClickEvent, _w, cx| cx.emit(FontsEvent::OpenFolder))),
                    ),
            )
            .into_any_element()
    }
}

/// Pages needed for `n` cards (at least one, so an empty list still shows page 1 of 1).
fn pages(n: usize) -> usize {
    n.div_ceil(PER_PAGE).max(1)
}

impl Render for FontsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = cx.global::<AppState>().user.is_some();
        let local_count = cx.global::<AppState>().fonts.list().len();
        let dir = cx.global::<AppState>().fonts.dir().display().to_string();
        let muted = cx.theme().muted_foreground;
        let cloud = self.tab > 0;
        let empty = match self.tab {
            0 => local_count == 0,
            1 => self.collected.is_empty(),
            _ => self.online.is_empty(),
        };

        let left = match self.tab {
            0 => div().text_xs().text_color(muted).truncate().child(format!("字体文件夹：{dir}（SVG 字体，复制文件即可分享）")).into_any_element(),
            1 if logged_in => Button::new("refresh")
                .ghost()
                .small()
                .icon(IconName::RefreshCw)
                .label("刷新")
                .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| this.refresh_collected(cx)))
                .into_any_element(),
            2 if logged_in => h_flex()
                .gap_2()
                .child(div().w(px(280.)).child(Input::new(&self.search).small().prefix(Icon::new(IconName::Search).small()).cleanable(true)))
                .child(Button::new("search").small().label("搜索").on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| this.request_online(1, cx))))
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let toolbar = h_flex()
            .h(px(32.))
            .gap_2()
            .child(div().flex_1().min_w_0().child(left))
            .when(!cloud || logged_in, |el| el.child(self.pager(cx)));

        let body = if cloud && !logged_in {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(div().text_color(muted).child("云端字体需要登录。下载过的字体会保存在本机，之后无需登录。"))
                .child(Button::new("login").primary().label("登录").on_click(cx.listener(|_this, _e: &ClickEvent, _w, cx| cx.emit(FontsEvent::Login))))
                .into_any_element()
        } else if empty {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(if self.tab == 0 { "字体库是空的：在「我的收藏」或「在线字体库」中下载，或「导入…」SVG 字体。" } else { "暂无字体" })
                .into_any_element()
        } else {
            self.grid(cx)
        };

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                TabBar::new("font-tabs")
                    .underline()
                    .selected_index(self.tab)
                    .children([Tab::new().label("本地字体"), Tab::new().label("我的收藏"), Tab::new().label("在线字体库")])
                    .on_click(cx.listener(move |this, ix: &usize, _w, cx| {
                        this.tab = *ix;
                        if *ix == 2 && logged_in && !this.online_requested {
                            this.request_online(1, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(toolbar)
            .when(self.tab == 0, |el| el.child(self.library_bar(cx)))
            .child(body)
    }
}
