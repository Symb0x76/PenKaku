//! The font library: handwriting fonts as plain SVG font files in folders the user can find,
//! back up and share. One default folder (`Documents/chixie-rs/Fonts`, movable) plus any number
//! of extra folders (a network share, a USB stick). Folders are scanned, not indexed, so a file
//! dropped into a folder is a font; [`watch`] reports changes while the app runs.
//!
//! System TTF/OTF fonts are deliberately not scanned: they are filled outlines, not pen strokes.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Font;
use super::svg::{self, FontMeta, FontSource};
use crate::font_store::FontStore;
use crate::{Error, Result};

/// Identifies a font across folders and machines: `cloud:<id>` for a ChiXie cloud font, else
/// `sha:<12 hex>` of the file's bytes (the same file is the same font wherever it sits).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FontKey(pub String);

impl FontKey {
    pub fn cloud(id: i64) -> Self {
        Self(format!("cloud:{id}"))
    }

    pub fn cloud_id(&self) -> Option<i64> {
        self.0.strip_prefix("cloud:")?.parse().ok()
    }

    fn of(meta: &FontMeta, bytes: &[u8]) -> Self {
        match meta.source {
            FontSource::Cloud { id } => Self::cloud(id),
            _ => {
                let h = Sha256::digest(bytes);
                Self(format!("sha:{}", h.iter().take(6).map(|b| format!("{b:02x}")).collect::<String>()))
            }
        }
    }
}

impl std::fmt::Display for FontKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One font found in the library.
#[derive(Debug, Clone, PartialEq)]
pub struct LibEntry {
    pub key: FontKey,
    pub meta: FontMeta,
    pub chars: usize,
    /// The file used (first folder in scan order: default folder, then extra folders).
    pub path: PathBuf,
    /// Other copies of the same font in other folders.
    pub copies: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontLibrary {
    pub default_dir: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
}

/// `Documents/chixie-rs/Fonts` (Windows `%USERPROFILE%\Documents`, macOS `~/Documents`, Linux
/// `$XDG_DOCUMENTS_DIR`); the home folder when there is no documents folder.
pub fn default_dir() -> PathBuf {
    let dirs = directories::UserDirs::new();
    let docs = dirs.as_ref().and_then(|d| d.document_dir().map(Path::to_path_buf)).or_else(|| dirs.as_ref().map(|d| d.home_dir().to_path_buf()));
    docs.unwrap_or_else(|| PathBuf::from(".")).join("chixie-rs").join("Fonts")
}

impl FontLibrary {
    pub fn new(default_dir: PathBuf, extra_dirs: Vec<PathBuf>) -> Self {
        Self { default_dir, extra_dirs }
    }

    fn dirs(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.default_dir).chain(&self.extra_dirs)
    }

    /// Every font in every folder, sorted by name; a font found twice is listed once with its
    /// other copies. Unreadable files are skipped.
    pub fn scan(&self) -> Vec<LibEntry> {
        let mut by_key: BTreeMap<FontKey, LibEntry> = BTreeMap::new();
        for dir in self.dirs() {
            for path in svg_files(dir) {
                let Ok(bytes) = std::fs::read(&path) else { continue };
                let Some((meta, chars)) = svg::read_meta(&bytes) else { continue };
                let key = FontKey::of(&meta, &bytes);
                match by_key.get_mut(&key) {
                    Some(e) => e.copies.push(path),
                    None => {
                        by_key.insert(key.clone(), LibEntry { key, meta, chars, path, copies: Vec::new() });
                    }
                }
            }
        }
        let mut v: Vec<LibEntry> = by_key.into_values().collect();
        v.sort_by(|a, b| a.meta.name.cmp(&b.meta.name).then_with(|| a.key.cmp(&b.key)));
        v
    }

    pub fn find(&self, key: &FontKey) -> Option<LibEntry> {
        self.scan().into_iter().find(|e| &e.key == key)
    }

    /// Load a font's glyphs (from its first copy that parses).
    pub fn load(&self, key: &FontKey) -> Result<(Font, FontMeta)> {
        let e = self.find(key).ok_or_else(|| Error::Font(format!("font {key} is not in the library")))?;
        let mut last = Error::Font(format!("font {key}: no readable copy"));
        for p in std::iter::once(&e.path).chain(&e.copies) {
            match std::fs::read(p).map_err(Error::from).and_then(|b| svg::read(&b)) {
                Ok(f) => return Ok(f),
                Err(err) => last = err,
            }
        }
        Err(last)
    }

    /// Save a downloaded cloud font into the default folder as `<name>.svg` (replacing an older
    /// copy of the same font there). Returns the file.
    pub fn save_cloud(&self, id: i64, name: &str, font: &Font) -> Result<PathBuf> {
        let meta = FontMeta { name: name.to_string(), source: FontSource::Cloud { id } };
        let key = FontKey::cloud(id);
        let existing = svg_files(&self.default_dir).into_iter().find(|p| std::fs::read(p).ok().and_then(|b| svg::read_meta(&b).map(|(m, _)| FontKey::of(&m, &b))).as_ref() == Some(&key));
        let path = existing.unwrap_or_else(|| unique_path(&self.default_dir, name, Some(id)));
        write_atomic(&path, svg::write(font, &meta).as_bytes())?;
        Ok(path)
    }

    /// Copy `.svg` font files, or the `.svg` files inside `.zip` archives, into the default folder.
    /// Files that are not SVG fonts are skipped. Returns the keys of the imported fonts.
    pub fn import(&self, paths: &[PathBuf]) -> Result<Vec<FontKey>> {
        let mut keys = Vec::new();
        let mut add = |name: &str, bytes: Vec<u8>| -> Result<()> {
            let Some((meta, _)) = svg::read_meta(&bytes) else { return Ok(()) };
            let key = FontKey::of(&meta, &bytes);
            if self.find(&key).is_none() {
                let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or("font");
                write_atomic(&unique_path(&self.default_dir, stem, None), &bytes)?;
            }
            keys.push(key);
            Ok(())
        };
        for p in paths {
            let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            match ext.as_deref() {
                Some("svg") => add(&p.file_name().unwrap_or_default().to_string_lossy(), std::fs::read(p)?)?,
                Some("zip") => {
                    let mut z = zip::ZipArchive::new(std::fs::File::open(p)?).map_err(|e| Error::Font(format!("zip: {e}")))?;
                    for i in 0..z.len() {
                        let mut f = z.by_index(i).map_err(|e| Error::Font(format!("zip: {e}")))?;
                        let name = f.name().map_err(|e| Error::Font(format!("zip: {e}")))?.to_string();
                        if !name.to_ascii_lowercase().ends_with(".svg") || f.is_dir() {
                            continue;
                        }
                        let mut bytes = Vec::new();
                        f.read_to_end(&mut bytes)?;
                        add(&name, bytes)?;
                    }
                }
                _ => {}
            }
        }
        Ok(keys)
    }

    /// Export fonts: one font to `dest` as an `.svg`; several (or a `.zip` dest) as a ZIP.
    pub fn export(&self, keys: &[FontKey], dest: &Path) -> Result<()> {
        let all = self.scan();
        let files: Vec<&LibEntry> = keys.iter().filter_map(|k| all.iter().find(|e| &e.key == k)).collect();
        let zipped = dest.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if let ([one], false) = (files.as_slice(), zipped) {
            std::fs::copy(&one.path, dest)?;
            return Ok(());
        }
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut names = Vec::new();
        for e in files {
            let base = e.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let mut name = base.clone();
            for n in 2.. {
                if !names.contains(&name) {
                    break;
                }
                name = format!("{n}-{base}");
            }
            z.start_file(&name, opt).map_err(|e| Error::Font(format!("zip: {e}")))?;
            z.write_all(&std::fs::read(&e.path)?)?;
            names.push(name);
        }
        let bytes = z.finish().map_err(|e| Error::Font(format!("zip: {e}")))?.into_inner();
        write_atomic(dest, &bytes)
    }

    /// Delete the font's file in the default folder (copies in extra folders belong to whoever
    /// shares that folder and are left alone). Returns whether a file was deleted.
    pub fn remove(&self, key: &FontKey) -> Result<bool> {
        let Some(e) = self.find(key) else { return Ok(false) };
        let mine: Vec<&PathBuf> = std::iter::once(&e.path).chain(&e.copies).filter(|p| p.parent() == Some(self.default_dir.as_path())).collect();
        for p in &mine {
            std::fs::remove_file(p)?;
        }
        Ok(!mine.is_empty())
    }

    /// Move every font of the default folder to `new_dir` (which becomes the default folder):
    /// copy, check each copy reads back, then delete the originals.
    pub fn move_default(&self, new_dir: &Path) -> Result<FontLibrary> {
        std::fs::create_dir_all(new_dir)?;
        let mut moved = Vec::new();
        for src in svg_files(&self.default_dir) {
            let bytes = std::fs::read(&src)?;
            let name = src.file_stem().and_then(|s| s.to_str()).unwrap_or("font");
            let dst = unique_path(new_dir, name, None);
            write_atomic(&dst, &bytes)?;
            if std::fs::read(&dst)? != bytes {
                return Err(Error::Font(format!("copy of {} differs; nothing deleted", src.display())));
            }
            moved.push(src);
        }
        for src in moved {
            std::fs::remove_file(src)?;
        }
        Ok(FontLibrary { default_dir: new_dir.to_path_buf(), extra_dirs: self.extra_dirs.clone() })
    }

    /// One-time migration from the pre-M4 store (`<config>/fonts/index.json` + `<id>.json`):
    /// every font becomes an SVG in the default folder, then the old folder is renamed
    /// `fonts.migrated` (kept, never deleted). Returns how many fonts moved; 0 when there is
    /// nothing to migrate.
    pub fn migrate_store(&self, old_dir: &Path) -> Result<usize> {
        if !old_dir.join("index.json").exists() {
            return Ok(0);
        }
        let store = FontStore::new(old_dir.to_path_buf());
        let mut n = 0;
        for f in store.list() {
            if let Ok(font) = store.load(f.id) {
                self.save_cloud(f.id, &f.name, &font)?;
                n += 1;
            }
        }
        let mut target = old_dir.with_file_name("fonts.migrated");
        for k in 2.. {
            if !target.exists() {
                break;
            }
            target = old_dir.with_file_name(format!("fonts.migrated.{k}"));
        }
        std::fs::rename(old_dir, target)?;
        Ok(n)
    }
}

fn svg_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> =
        rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))).collect();
    v.sort();
    v
}

/// A file name every OS accepts: no path separators or reserved characters, no trailing dots.
fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').trim().to_string();
    if s.is_empty() { "font".into() } else { s }
}

/// `<dir>/<name>.svg`, or `<name> (<id>).svg` / `<name> 2.svg`… when that file exists.
fn unique_path(dir: &Path, name: &str, id: Option<i64>) -> PathBuf {
    let base = sanitize(name);
    let first = dir.join(format!("{base}.svg"));
    if !first.exists() {
        return first;
    }
    if let Some(id) = id {
        let p = dir.join(format!("{base} ({id}).svg"));
        if !p.exists() {
            return p;
        }
    }
    (2..).map(|k| dir.join(format!("{base} {k}.svg"))).find(|p| !p.exists()).expect("some name is free")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("svg.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Watches library folders; dropping it stops watching.
pub struct LibraryWatcher {
    _watcher: notify::RecommendedWatcher,
}

/// Call `on_change` (on a background thread) after files in `dirs` change, at most once per
/// burst: events are gathered for 300 ms first. Folders that do not exist are skipped.
pub fn watch(dirs: &[PathBuf], on_change: impl Fn() + Send + 'static) -> notify::Result<LibraryWatcher> {
    use notify::Watcher as _;
    let (tx, rx) = mpsc::channel();
    let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        if ev.is_ok_and(|e| !e.kind.is_access()) {
            let _ = tx.send(());
        }
    })?;
    for d in dirs.iter().filter(|d| d.is_dir()) {
        w.watch(d, notify::RecursiveMode::NonRecursive)?;
    }
    std::thread::spawn(move || {
        // Ends when the watcher (and so the sender) is dropped.
        while rx.recv().is_ok() {
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            on_change();
        }
    });
    Ok(LibraryWatcher { _watcher: w })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Glyph;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chixie-rs-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn font(c: char) -> Font {
        Font { glyphs: [(c, Glyph { advance: 7.0, strokes: vec![vec![(0.0, 0.0), (7.0, 7.0)]] })].into_iter().collect() }
    }

    fn imported(dir: &Path, file: &str, c: char) -> PathBuf {
        let p = dir.join(file);
        std::fs::write(&p, svg::write(&font(c), &FontMeta { name: file.into(), source: FontSource::Imported })).unwrap();
        p
    }

    #[test]
    fn scan_save_load_and_dedupe() {
        let (a, b) = (tmp("scan-a"), tmp("scan-b"));
        let lib = FontLibrary::new(a.clone(), vec![b.clone()]);
        let p = lib.save_cloud(7, "雅/体?", &font('你')).unwrap();
        assert_eq!(p.file_name().unwrap(), "雅_体_.svg");
        // Same cloud font in the extra folder, and an unrelated file.
        std::fs::copy(&p, b.join("copy.svg")).unwrap();
        std::fs::write(b.join("notes.svg"), "<svg/>").unwrap();
        imported(&b, "mine.svg", '好');
        let all = lib.scan();
        assert_eq!(all.len(), 2, "{all:?}");
        let cloud = all.iter().find(|e| e.key == FontKey::cloud(7)).unwrap();
        assert_eq!((cloud.chars, cloud.copies.len(), cloud.path.parent()), (1, 1, Some(a.as_path())));
        let (f, m) = lib.load(&FontKey::cloud(7)).unwrap();
        assert_eq!((f, m.name.as_str()), (font('你'), "雅/体?"));
        // Saving again replaces the same file.
        lib.save_cloud(7, "雅/体?", &font('好')).unwrap();
        assert_eq!(svg_files(&a).len(), 1);
        assert!(lib.load(&FontKey::cloud(7)).unwrap().0.get('好').is_some());
    }

    #[test]
    fn import_export_zip_and_remove() {
        let (src, lib_dir, out) = (tmp("imp-src"), tmp("imp-lib"), tmp("imp-out"));
        let lib = FontLibrary::new(lib_dir.clone(), vec![]);
        let one = imported(&src, "one.svg", '一');
        let two = imported(&src, "two.svg", '二');
        let keys = lib.import(&[one.clone(), two]).unwrap();
        assert_eq!(keys.len(), 2);
        // Importing the same file again adds nothing.
        lib.import(&[one]).unwrap();
        assert_eq!(svg_files(&lib_dir).len(), 2);
        let zip = out.join("fonts.zip");
        lib.export(&keys, &zip).unwrap();
        let other = FontLibrary::new(tmp("imp-other"), vec![]);
        assert_eq!(other.import(&[zip]).unwrap().len(), 2);
        assert_eq!(other.scan().iter().map(|e| e.key.clone()).collect::<Vec<_>>().len(), 2);
        let single = out.join("one.svg");
        lib.export(&keys[..1], &single).unwrap();
        assert!(svg::read_meta(&std::fs::read(&single).unwrap()).is_some());
        assert!(lib.remove(&keys[0]).unwrap());
        assert_eq!(lib.scan().len(), 1);
    }

    #[test]
    fn moving_the_default_folder_keeps_every_font() {
        let (old, new) = (tmp("move-old"), tmp("move-new"));
        let lib = FontLibrary::new(old.clone(), vec![]);
        lib.save_cloud(1, "甲", &font('甲')).unwrap();
        imported(&old, "乙.svg", '乙');
        let keys: Vec<FontKey> = lib.scan().into_iter().map(|e| e.key).collect();
        let moved = lib.move_default(&new).unwrap();
        assert!(svg_files(&old).is_empty());
        for k in &keys {
            assert!(moved.load(k).is_ok(), "{k}");
        }
    }

    #[test]
    fn migrates_the_old_store_once() {
        let root = tmp("migrate");
        let old = root.join("fonts");
        let store = FontStore::new(old.clone());
        store.save(5, "旧字体", &font('旧')).unwrap();
        let lib = FontLibrary::new(root.join("lib"), vec![]);
        assert_eq!(lib.migrate_store(&old).unwrap(), 1);
        assert!(!old.exists() && root.join("fonts.migrated").exists());
        assert_eq!(lib.load(&FontKey::cloud(5)).unwrap().1.name, "旧字体");
        assert_eq!(lib.migrate_store(&old).unwrap(), 0, "nothing left to migrate");
    }

    #[test]
    fn a_file_dropped_into_a_watched_folder_shows_up_within_2_s() {
        let (a, extra) = (tmp("watch-a"), tmp("watch-b"));
        let lib = FontLibrary::new(a.clone(), vec![extra.clone()]);
        let (tx, rx) = mpsc::channel();
        let _w = watch(&[a, extra.clone()], move || {
            let _ = tx.send(());
        })
        .unwrap();
        let t0 = std::time::Instant::now();
        imported(&extra, "dropped.svg", '新');
        rx.recv_timeout(Duration::from_secs(2)).expect("change reported within 2 s");
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert_eq!(lib.scan().len(), 1);
    }
}
