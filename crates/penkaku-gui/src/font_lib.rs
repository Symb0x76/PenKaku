//! The font library as the app sees it: fonts by number (`FontKey::local_id`: the cloud id, or a
//! negative hash for imported fonts), served from a cached scan of the library folders. The
//! cache is refreshed after every change made here and when the folder watcher reports one.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use penkaku_core::Result;
use penkaku_core::font::Font;
use penkaku_core::font::library::{FontKey, FontLibrary, LibEntry};
use penkaku_core::font::svg::FontSource;

/// One font of the library.
#[derive(Debug, Clone)]
pub struct LocalFont {
    pub id: i64,
    pub name: String,
    pub glyphs: usize,
    pub path: PathBuf,
    pub source: FontSource,
    /// The same font also sits in this many other folders.
    pub copies: usize,
}

impl From<&LibEntry> for LocalFont {
    fn from(e: &LibEntry) -> Self {
        Self { id: e.key.local_id(), name: e.meta.name.clone(), glyphs: e.chars, path: e.path.clone(), source: e.meta.source.clone(), copies: e.copies.len() }
    }
}

pub struct LibraryStore {
    lib: RwLock<FontLibrary>,
    entries: RwLock<Vec<LibEntry>>,
    /// App cache folder (cloud preview images live in `previews/` under it).
    cache_dir: PathBuf,
}

impl LibraryStore {
    pub fn new(lib: FontLibrary, cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&lib.default_dir);
        let entries = lib.scan();
        Self { lib: RwLock::new(lib), entries: RwLock::new(entries), cache_dir }
    }

    pub fn library(&self) -> FontLibrary {
        self.lib.read().expect("library lock").clone()
    }

    /// Use other folders (after 选项 › 字体 changes) and rescan.
    pub fn set_library(&self, lib: FontLibrary) {
        let _ = std::fs::create_dir_all(&lib.default_dir);
        *self.lib.write().expect("library lock") = lib;
        self.rescan();
    }

    pub fn rescan(&self) {
        let entries = self.library().scan();
        *self.entries.write().expect("entries lock") = entries;
    }

    pub fn list(&self) -> Vec<LocalFont> {
        self.entries.read().expect("entries lock").iter().map(LocalFont::from).collect()
    }

    pub fn get(&self, id: i64) -> Option<LocalFont> {
        self.entries.read().expect("entries lock").iter().find(|e| e.key.local_id() == id).map(LocalFont::from)
    }

    pub fn contains(&self, id: i64) -> bool {
        self.get(id).is_some()
    }

    pub fn name_of(&self, id: i64) -> Option<String> {
        self.get(id).map(|f| f.name)
    }

    /// The first local font called `name`.
    pub fn id_of(&self, name: &str) -> Option<i64> {
        self.entries.read().expect("entries lock").iter().map(LocalFont::from).find(|f| f.name == name).map(|f| f.id)
    }

    /// The default library folder.
    pub fn dir(&self) -> PathBuf {
        self.library().default_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Save a downloaded cloud font.
    pub fn save(&self, id: i64, name: &str, font: &Font) -> Result<()> {
        self.library().save_cloud(id, name, font)?;
        self.rescan();
        Ok(())
    }

    /// Give a cloud font its real name (old stores saved placeholders). `false` when it is not a
    /// cloud font in the library.
    pub fn rename(&self, id: i64, name: &str) -> Result<bool> {
        let key = FontKey::from_local_id(id);
        if key.cloud_id().is_none() || !self.contains(id) {
            return Ok(false);
        }
        let (font, _) = self.library().load(&key)?;
        self.save(id, name, &font)?;
        Ok(true)
    }

    pub fn load(&self, id: i64) -> Result<Font> {
        Ok(self.library().load(&FontKey::from_local_id(id))?.0)
    }

    pub fn remove(&self, id: i64) -> Result<()> {
        self.library().remove(&FontKey::from_local_id(id))?;
        self.rescan();
        Ok(())
    }

    /// Import `.svg` / `.zip` files; returns the ids of the fonts found in them.
    pub fn import(&self, paths: &[PathBuf]) -> Result<Vec<i64>> {
        let keys = self.library().import(paths)?;
        self.rescan();
        Ok(keys.iter().map(FontKey::local_id).collect())
    }

    pub fn export(&self, ids: &[i64], dest: &Path) -> Result<()> {
        let keys: Vec<FontKey> = ids.iter().map(|&id| FontKey::from_local_id(id)).collect();
        self.library().export(&keys, dest)
    }

    /// Move the default folder's fonts to `new_dir`; returns the new library.
    pub fn move_default(&self, new_dir: &Path) -> Result<FontLibrary> {
        let moved = self.library().move_default(new_dir)?;
        self.set_library(moved.clone());
        Ok(moved)
    }
}
