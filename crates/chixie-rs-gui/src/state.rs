//! App-wide state shared by all views, stored as a gpui global. Views observe it with
//! `cx.observe_global::<AppState>` and re-render; mutate it via `cx.global_mut::<AppState>()`
//! (which notifies observers).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chixie_rs_core::config::Settings;
use chixie_rs_core::font::Font;
use chixie_rs_core::font::fontset::FontSet;
use chixie_rs_core::session::SessionHandle;
use chixie_rs_template::{Template, TemplateStore};
use gpui_kit::Global;

use crate::cloud_preview::CloudPreview;
use crate::font_preview::FontPreview;

pub struct User {
    pub name: String,
    pub role: String,
}

/// The decrypted font currently used for writing (memory only; tied to this login).
pub struct ActiveFont {
    pub id: i64,
    pub name: String,
    pub font: Arc<Font>,
}

pub struct AppState {
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub session: SessionHandle,
    pub user: Option<User>,
    /// The main (主线) font, for names in the UI.
    pub active_font: Option<ActiveFont>,
    /// Every font writing uses: roles, mix and fallbacks (see `ChixieRsApp::reload_fonts`).
    pub font_set: Arc<FontSet>,
    /// Bumped whenever `font_set` changes, so views know to lay out again.
    pub font_set_gen: u64,
    /// Fonts named by characters' own 字体 (`None`: no such local font), loaded on first use.
    pub run_fonts: HashMap<String, Option<Arc<Font>>>,
    pub templates: TemplateStore,
    /// Where templates are kept: one SVG each (`Documents/chixie-rs/Templates` + extra folders).
    pub template_lib: chixie_rs_template::library::TemplateLibrary,
    /// Downloaded fonts kept on this computer (work without login).
    pub fonts: std::sync::Arc<crate::font_lib::LibraryStore>,
    pub templates_path: PathBuf,
    /// Font picker previews by font id (built from local fonts in the background).
    pub previews: HashMap<i64, Arc<FontPreview>>,
    /// Server preview images of fonts that are not downloaded.
    pub cloud_previews: HashMap<i64, CloudPreview>,
}

impl Global for AppState {}

impl AppState {
    /// Persist settings; failures are logged, never fatal for the UI.
    pub fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            log::warn!("saving settings to {}: {e}", self.settings_path.display());
        }
    }

    /// The page template in use (`settings.page.template`), if it still exists.
    pub fn active_template(&self) -> Option<Template> {
        self.settings.page.template.as_ref().and_then(|n| self.templates.get(n)).cloned()
    }

    /// Write the templates to the library's default folder (one SVG each; deleted ones removed).
    pub fn save_templates(&self) {
        if let Err(e) = self.template_lib.sync(&self.templates) {
            log::warn!("saving templates to {}: {e}", self.template_lib.default_dir.display());
        }
    }

    /// Read the templates again from the library folders.
    pub fn reload_templates(&mut self) {
        let scanned: Vec<Template> = self.template_lib.scan().into_iter().map(|e| e.template).collect();
        self.templates = TemplateStore { templates: scanned };
    }
}
