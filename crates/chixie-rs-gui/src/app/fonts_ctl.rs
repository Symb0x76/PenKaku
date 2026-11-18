//! Fonts in use: the library folders (watched), the role fonts (主线 / 英文 / 符号 / 数字), 字体混用
//! and 字体优化. Everything writing needs is gathered into `AppState::font_set`.

use std::path::PathBuf;
use std::sync::Arc;

use chixie_rs_core::font::Font;
use chixie_rs_core::font::fix::{self, FontFix};
use chixie_rs_core::font::fontset::{FontRoles, FontSet, MixEntry};
use chixie_rs_core::font::library::{self, FontKey, LibraryWatcher};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::font_preview::FontPreview;
use crate::state::{ActiveFont, AppState};

/// Which role a font plays (字体库 card menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Main,
    English,
    Symbol,
    Number,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Main, Role::English, Role::Symbol, Role::Number];

    pub fn label(self) -> &'static str {
        match self {
            Role::Main => "主线字体",
            Role::English => "英文字体",
            Role::Symbol => "符号字体",
            Role::Number => "数字字体",
        }
    }

    pub fn slot(self, r: &mut FontRoles) -> &mut Option<FontKey> {
        match self {
            Role::Main => &mut r.main,
            Role::English => &mut r.english,
            Role::Symbol => &mut r.symbol,
            Role::Number => &mut r.number,
        }
    }

    pub fn get(self, r: &FontRoles) -> Option<&FontKey> {
        match self {
            Role::Main => r.main.as_ref(),
            Role::English => r.english.as_ref(),
            Role::Symbol => r.symbol.as_ref(),
            Role::Number => r.number.as_ref(),
        }
    }
}

/// Keeps the library folders watched while the app runs.
pub struct LibraryWatch {
    _watcher: LibraryWatcher,
    _task: Task<()>,
}

impl ChixieRsApp {
    /// Load every font the roles and the mix name (applying 字体优化) and rebuild the font set.
    /// Fonts not in the library are skipped (a cloud main font is downloaded at login).
    pub(crate) fn reload_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        let f = st.settings.fonts.clone();
        let store = st.fonts.clone();
        let mut keys = f.roles.keys();
        for m in &f.mix {
            if !keys.contains(&m.key) {
                keys.push(m.key.clone());
            }
        }
        let fixes = f.fix;
        let task = cx.background_executor().spawn(async move {
            keys.into_iter()
                .filter_map(|k| {
                    let id = k.local_id();
                    let font = store.load(id).inspect_err(|e| log::warn!("loading font {k}: {e}")).ok()?;
                    let font = if fixes.any() { fix::apply(&font, &fixes) } else { font };
                    Some((k, Arc::new(font), store.name_of(id).unwrap_or_default()))
                })
                .collect::<Vec<(FontKey, Arc<Font>, String)>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                let st = cx.global_mut::<AppState>();
                let f = st.settings.fonts.clone();
                let pairs: Vec<(FontKey, Arc<Font>)> = loaded.iter().map(|(k, font, _)| (k.clone(), font.clone())).collect();
                st.font_set = Arc::new(FontSet::build(&pairs, &f.roles, &f.mix, f.mix_salt));
                st.font_set_gen += 1;
                let main = f.roles.main.as_ref().and_then(|m| loaded.iter().find(|(k, ..)| k == m)).or(loaded.first());
                st.active_font = main.map(|(k, font, name)| ActiveFont { id: k.local_id(), name: name.clone(), font: font.clone() });
                if let Some(a) = &st.active_font
                    && !st.previews.contains_key(&a.id)
                    && let Some(p) = FontPreview::new(&a.font, &a.name)
                {
                    st.previews.insert(a.id, Arc::new(p));
                }
                app.sync_font_select(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Make a library font the main font (字体库 使用, the ribbon picker).
    pub(crate) fn use_main_font(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.set_role(id, Role::Main, window, cx);
        if window.has_active_dialog(cx) {
            window.close_dialog(cx);
        }
    }

    pub(crate) fn set_role(&mut self, id: i64, role: Role, window: &mut Window, cx: &mut Context<Self>) {
        let key = FontKey::from_local_id(id);
        self.edit_settings(cx, |st| {
            *role.slot(&mut st.settings.fonts.roles) = Some(key.clone());
            if role == Role::Main {
                // Older builds read this; keep it for cloud fonts.
                st.settings.page.font_id = key.cloud_id();
            }
        });
        self.reload_fonts(window, cx);
    }

    /// Stop using a font for a role (the main role always keeps a font).
    pub(crate) fn clear_role(&mut self, role: Role, window: &mut Window, cx: &mut Context<Self>) {
        if role != Role::Main {
            self.edit_settings(cx, |st| *role.slot(&mut st.settings.fonts.roles) = None);
            self.reload_fonts(window, cx);
        }
    }

    /// 加入混用 / 移出混用.
    pub(crate) fn toggle_mix(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let key = FontKey::from_local_id(id);
        self.edit_settings(cx, |st| {
            let mix = &mut st.settings.fonts.mix;
            match mix.iter().position(|m| m.key == key) {
                Some(i) => {
                    mix.remove(i);
                }
                None => mix.push(MixEntry { key: key.clone(), weight: 1.0 }),
            }
        });
        self.reload_fonts(window, cx);
    }

    pub(crate) fn set_mix_weight(&mut self, id: i64, weight: f32, window: &mut Window, cx: &mut Context<Self>) {
        let key = FontKey::from_local_id(id);
        self.edit_settings(cx, |st| {
            if let Some(m) = st.settings.fonts.mix.iter_mut().find(|m| m.key == key) {
                m.weight = weight.clamp(0.0, 100.0);
            }
        });
        self.reload_fonts(window, cx);
    }

    /// 换一组: deal the mixed fonts out to the characters again.
    pub(crate) fn reroll_mix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_settings(cx, |st| st.settings.fonts.mix_salt = st.settings.fonts.mix_salt.wrapping_add(0x9E37_79B9));
        self.reload_fonts(window, cx);
    }

    pub(crate) fn set_font_fix(&mut self, fix: FontFix, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_settings(cx, |st| st.settings.fonts.fix = fix);
        self.reload_fonts(window, cx);
    }

    /// Watch the library folders; a change rescans them and reloads the fonts in use.
    pub(crate) fn watch_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lib = cx.global::<AppState>().fonts.library();
        let dirs: Vec<PathBuf> = std::iter::once(lib.default_dir.clone()).chain(lib.extra_dirs.clone()).collect();
        let (tx, rx) = async_channel::unbounded::<()>();
        let watcher = match library::watch(&dirs, move || {
            let _ = tx.try_send(());
        }) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("watching font folders: {e}");
                self.library_watch = None;
                return;
            }
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while rx.recv().await.is_ok() {
                if this.update_in(cx, |app, window, cx| app.library_changed(window, cx)).is_err() {
                    break;
                }
            }
        });
        self.library_watch = Some(LibraryWatch { _watcher: watcher, _task: task });
    }

    /// Files in a library folder changed (or the folders did).
    pub(crate) fn library_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = cx.global::<AppState>().fonts.clone();
        let task = cx.background_executor().spawn(async move { store.rescan() });
        cx.spawn_in(window, async move |this, cx| {
            task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.sync_font_select(window, cx);
                app.fonts.update(cx, |_, cx| cx.notify());
                app.reload_fonts(window, cx);
            });
        })
        .detach();
    }

    /// 导入字体: `.svg` font files or `.zip` archives of them.
    pub(crate) fn import_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("导入字体（.svg / .zip）".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = this.update_in(cx, |app, window, cx| {
                match cx.global::<AppState>().fonts.import(&paths) {
                    Ok(ids) if ids.is_empty() => window.push_notification(Notification::warning("没有找到 SVG 字体"), cx),
                    Ok(ids) => window.push_notification(Notification::success(format!("已导入 {} 个字体", ids.len())), cx),
                    Err(e) => window.push_notification(Notification::error(format!("导入失败：{e}")), cx),
                }
                app.library_changed(window, cx);
            });
        })
        .detach();
    }

    /// 导出: one font as an `.svg` file.
    pub(crate) fn export_font(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        let name = st.fonts.name_of(id).unwrap_or_else(|| "字体".into());
        let rx = cx.prompt_for_new_path(&crate::file_ops::documents_dir(), Some(&format!("{name}.svg")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let _ = this.update_in(cx, |_app, window, cx| match cx.global::<AppState>().fonts.export(&[id], &path) {
                Ok(()) => window.push_notification(Notification::success(format!("已导出到 {}", path.display())), cx),
                Err(e) => window.push_notification(Notification::error(format!("导出失败：{e}")), cx),
            });
        })
        .detach();
    }

    /// 在文件夹中显示.
    pub(crate) fn reveal_font(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = cx.global::<AppState>().fonts.get(id).map(|f| f.path) else { return };
        if let Err(e) = reveal(&path) {
            window.push_notification(Notification::error(format!("无法打开文件夹：{e}")), cx);
        }
    }

    pub(crate) fn open_font_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = cx.global::<AppState>().fonts.dir();
        if let Err(e) = open_dir(&dir) {
            window.push_notification(Notification::error(format!("无法打开文件夹：{e}")), cx);
        }
    }

    /// 缺失字符: the characters no font in use has, with 复制.
    pub(crate) fn open_missing_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing: String = self.doc.read(cx).document().missing.iter().collect();
        let font = cx.global::<AppState>().active_font.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        window.open_dialog(cx, move |dialog, _w, cx| {
            let muted = gpui_kit::component::ActiveTheme::theme(cx).muted_foreground;
            let text = missing.clone();
            dialog
                .title("缺失字符")
                .w(px(460.))
                .child(
                    gpui_kit::component::v_flex()
                        .gap_3()
                        .child(div().text_sm().text_color(muted).child(format!("当前所用字体（{font} 及其他角色、混用字体）都没有这些字，它们在页面上显示为虚线框，不会写出：")))
                        .child(div().p_2().rounded_md().border_1().text_lg().child(missing.clone()))
                        .child(
                            gpui_kit::component::button::Button::new("missing-copy").label("复制").on_click(move |_: &ClickEvent, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                window.push_notification(Notification::success("已复制"), cx);
                            }),
                        ),
                )
        });
    }

    /// 选项 › 字体 › 更改…: move the default folder (fonts move with it).
    pub(crate) fn change_library_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("选择字体文件夹".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |app, window, cx| match cx.global::<AppState>().fonts.move_default(&dir) {
                Ok(lib) => {
                    app.edit_settings(cx, |st| st.settings.fonts.library_dir = Some(lib.default_dir.clone()));
                    window.push_notification(Notification::success(format!("字体文件夹已移到 {}", lib.default_dir.display())), cx);
                    app.watch_library(window, cx);
                    app.library_changed(window, cx);
                }
                Err(e) => window.push_notification(Notification::error(format!("移动失败，字体仍在原文件夹：{e}")), cx),
            });
        })
        .detach();
    }

    /// 选项 › 字体 › 添加…: one more folder to read fonts from.
    pub(crate) fn add_library_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("添加字体文件夹".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = this.update_in(cx, |app, window, cx| {
                app.edit_settings(cx, |st| {
                    for p in paths {
                        if !st.settings.fonts.extra_dirs.contains(&p) {
                            st.settings.fonts.extra_dirs.push(p);
                        }
                    }
                });
                app.apply_library_dirs(window, cx);
            });
        })
        .detach();
    }

    pub(crate) fn remove_library_dir(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_settings(cx, |st| {
            if ix < st.settings.fonts.extra_dirs.len() {
                st.settings.fonts.extra_dirs.remove(ix);
            }
        });
        self.apply_library_dirs(window, cx);
    }

    fn apply_library_dirs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = cx.global::<AppState>();
        st.fonts.set_library(st.settings.fonts.library());
        self.watch_library(window, cx);
        self.library_changed(window, cx);
    }
}

/// Show `path` selected in the system file manager.
fn reveal(path: &std::path::Path) -> std::io::Result<()> {
    if cfg!(target_os = "windows") {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map(drop)
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn().map(drop)
    } else {
        open_dir(path.parent().unwrap_or(path))
    }
}

pub(crate) fn open_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let cmd = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd).arg(dir).spawn().map(drop)
}
