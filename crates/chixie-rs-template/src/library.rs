//! The template library: one SVG file per template (see [`crate::svg`]) in a folder users can find,
//! back up and share — `Documents/chixie-rs/Templates` by default (movable) plus any number of
//! extra folders. Like the font library it is scanned, not indexed: a file dropped in is a
//! template. Only the default folder is written; templates in extra folders are read-only.
//!
//! Page backgrounds are embedded in the SVGs and kept as PNGs in `assets` while the app runs.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chixie_rs_core::{Error, Result};

use crate::template::{Template, TemplateKind, TemplateStore};

/// A template found in a library folder.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub template: Template,
    pub modified: SystemTime,
    /// In the default folder (can be saved over and deleted).
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateLibrary {
    pub default_dir: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
    /// Where page background PNGs are kept while in use.
    pub assets: PathBuf,
}

/// `Documents/chixie-rs/Templates` (the home folder when there is no documents folder).
pub fn default_dir() -> PathBuf {
    let dirs = directories::UserDirs::new();
    let docs = dirs.as_ref().and_then(|d| d.document_dir().map(Path::to_path_buf)).or_else(|| dirs.as_ref().map(|d| d.home_dir().to_path_buf()));
    docs.unwrap_or_else(|| PathBuf::from(".")).join("chixie-rs").join("Templates")
}

fn err(e: impl std::fmt::Display) -> Error {
    Error::Config(e.to_string())
}

fn svg_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))).collect();
    v.sort();
    v
}

/// A file name for `name`: characters Windows forbids become `_`.
fn file_stem(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() { "模板".into() } else { s }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("svg.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl TemplateLibrary {
    pub fn new(default_dir: PathBuf, extra_dirs: Vec<PathBuf>, assets: PathBuf) -> Self {
        Self { default_dir, extra_dirs, assets }
    }

    /// Every folder, the default one first (it is also where changes are written).
    pub fn dirs(&self) -> Vec<PathBuf> {
        std::iter::once(self.default_dir.clone()).chain(self.extra_dirs.iter().cloned()).collect()
    }

    /// Every template in every folder; a name found twice is listed once (the default folder
    /// wins, then the first extra folder). Unreadable files are skipped.
    pub fn scan(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for (k, dir) in self.dirs().iter().enumerate() {
            for path in svg_files(dir) {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(template) = crate::svg::read(&text, &self.assets) else { continue };
                if out.iter().any(|e| e.template.name == template.name) {
                    continue;
                }
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                out.push(Entry { path, template, modified, editable: k == 0 });
            }
        }
        out
    }

    /// The file a template is saved to in the default folder.
    pub fn path_of(&self, name: &str) -> PathBuf {
        self.default_dir.join(format!("{}.svg", file_stem(name)))
    }

    /// Write (or overwrite) `t` in the default folder.
    pub fn save(&self, t: &Template) -> Result<PathBuf> {
        let path = self.path_of(&t.name);
        write_atomic(&path, crate::svg::write(t, &self.assets).as_bytes())?;
        Ok(path)
    }

    /// Delete the template's file from the default folder. Returns whether one was deleted.
    pub fn remove(&self, name: &str) -> Result<bool> {
        let path = self.path_of(name);
        if path.exists() {
            std::fs::remove_file(path)?;
            return Ok(true);
        }
        // A file named differently (dropped in by hand) that holds this template.
        match self.scan().into_iter().find(|e| e.editable && e.template.name == name) {
            Some(e) => {
                std::fs::remove_file(e.path)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Make the default folder hold exactly `store`'s editable templates: write each one,
    /// remove default-folder files of templates no longer in it. Extra folders are not touched.
    pub fn sync(&self, store: &TemplateStore) -> Result<()> {
        let extra: Vec<String> = self.scan().into_iter().filter(|e| !e.editable).map(|e| e.template.name).collect();
        for t in store.templates.iter().filter(|t| !extra.contains(&t.name)) {
            let path = self.path_of(&t.name);
            let svg = crate::svg::write(t, &self.assets);
            if std::fs::read_to_string(&path).ok().as_deref() != Some(svg.as_str()) {
                write_atomic(&path, svg.as_bytes())?;
            }
        }
        for e in self.scan().into_iter().filter(|e| e.editable) {
            if store.get(&e.template.name).is_none() {
                std::fs::remove_file(&e.path)?;
            }
        }
        Ok(())
    }

    fn unique(&self, base: &str, taken: &mut Vec<String>) -> String {
        let name = if taken.iter().any(|t| t == base) { (2..).map(|n| format!("{base} ({n})")).find(|n| !taken.contains(n)).expect("unbounded") } else { base.to_string() };
        taken.push(name.clone());
        name
    }

    /// Bring templates in: `.svg`, a `.zip` of them, an `.xlsx` (a table template per workbook) or
    /// ChiXie3's `.cx`. Names already in the library get a number. Returns the new names.
    pub fn import(&self, paths: &[PathBuf]) -> Result<Vec<String>> {
        let mut taken: Vec<String> = self.scan().into_iter().map(|e| e.template.name).collect();
        let mut added = Vec::new();
        let mut add = |mut t: Template, taken: &mut Vec<String>| -> Result<()> {
            t.name = self.unique(&t.name, taken);
            self.save(&t)?;
            added.push(t.name);
            Ok(())
        };
        for p in paths {
            let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("模板").to_string();
            match ext.as_deref() {
                Some("svg") => add(crate::svg::read(&std::fs::read_to_string(p)?, &self.assets).map_err(err)?, &mut taken)?,
                Some("zip") => {
                    let mut z = zip::ZipArchive::new(std::fs::File::open(p)?).map_err(err)?;
                    for i in 0..z.len() {
                        let mut f = z.by_index(i).map_err(err)?;
                        if f.is_dir() || !f.name().map_err(err)?.to_ascii_lowercase().ends_with(".svg") {
                            continue;
                        }
                        let mut text = String::new();
                        f.read_to_string(&mut text)?;
                        if let Ok(t) = crate::svg::read(&text, &self.assets) {
                            add(t, &mut taken)?;
                        }
                    }
                }
                Some("xlsx") => {
                    let tt = crate::xlsx_template::import(&std::fs::read(p)?).map_err(err)?;
                    let mut t = Template::table(&stem, crate::template::A4);
                    t.kind = TemplateKind::Table(tt);
                    add(t, &mut taken)?;
                }
                Some("cx") => {
                    for t in crate::chixie3_templates::import(&std::fs::read(p)?)? {
                        add(t, &mut taken)?;
                    }
                }
                _ => return Err(err(format!("{}：不支持的模板文件", p.display()))),
            }
        }
        Ok(added)
    }

    /// Export: one template to `dest` as `.svg` (or `.xlsx` for a table template when `dest`
    /// ends in .xlsx); several, or a `.zip` dest, as a ZIP of SVGs.
    pub fn export(&self, templates: &[Template], dest: &Path) -> Result<()> {
        let ext = dest.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        match (templates, ext.as_deref()) {
            ([t], Some("xlsx")) => {
                let TemplateKind::Table(tt) = &t.kind else { return Err(err("只有表格模板可以导出为 Excel")) };
                std::fs::write(dest, crate::xlsx_template::export(tt).map_err(err)?)?;
            }
            ([t], Some(e)) if e != "zip" => std::fs::write(dest, crate::svg::write(t, &self.assets))?,
            _ => {
                let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
                let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                let mut names: Vec<String> = Vec::new();
                for t in templates {
                    let name = self.unique(&format!("{}.svg", file_stem(&t.name)), &mut names);
                    z.start_file(&name, opt).map_err(err)?;
                    z.write_all(crate::svg::write(t, &self.assets).as_bytes())?;
                }
                std::fs::write(dest, z.finish().map_err(err)?.into_inner())?;
            }
        }
        Ok(())
    }

    /// Move the default folder's templates to `new_dir` (which becomes the default folder):
    /// copy, check each copy, then delete the originals.
    pub fn move_default(&self, new_dir: &Path) -> Result<TemplateLibrary> {
        std::fs::create_dir_all(new_dir)?;
        let mut moved = Vec::new();
        for src in svg_files(&self.default_dir) {
            let bytes = std::fs::read(&src)?;
            let dst = new_dir.join(src.file_name().unwrap_or_default());
            write_atomic(&dst, &bytes)?;
            if std::fs::read(&dst)? != bytes {
                return Err(err(format!("copy of {} differs; nothing deleted", src.display())));
            }
            moved.push(src);
        }
        for src in moved {
            std::fs::remove_file(src)?;
        }
        Ok(TemplateLibrary { default_dir: new_dir.to_path_buf(), ..self.clone() })
    }

    /// One-time migration from the pre-M7c `templates.toml`: each template becomes an SVG in the
    /// default folder and the file is renamed `templates.toml.bak` (kept). Returns how many moved.
    pub fn migrate_toml(&self, toml_path: &Path) -> Result<usize> {
        if !toml_path.exists() {
            return Ok(0);
        }
        let store = TemplateStore::load(toml_path)?;
        let mut taken: Vec<String> = self.scan().into_iter().map(|e| e.template.name).collect();
        let mut n = 0;
        for t in store.templates {
            if taken.contains(&t.name) {
                continue;
            }
            taken.push(t.name.clone());
            self.save(&t)?;
            n += 1;
        }
        std::fs::rename(toml_path, toml_path.with_extension("toml.bak"))?;
        Ok(n)
    }

    /// The templates to work with: the library's, after migrating `templates.toml` once; the
    /// built-ins are written when the library is empty.
    pub fn load_store(&self, old_toml: &Path) -> Result<TemplateStore> {
        self.migrate_toml(old_toml)?;
        let mut entries = self.scan();
        if entries.is_empty() {
            for t in TemplateStore::builtins().templates {
                self.save(&t)?;
            }
            entries = self.scan();
        }
        Ok(TemplateStore { templates: entries.into_iter().map(|e| e.template).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::ColWidth;
    use crate::template::A4;

    fn lib(name: &str) -> (TemplateLibrary, PathBuf) {
        let root = std::env::temp_dir().join(format!("chixie-rs-lib-test-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (TemplateLibrary::new(root.join("Templates"), vec![root.join("Shared")], root.join("assets")), root)
    }

    fn table() -> Template {
        let mut t = Template::table("体温表", A4);
        if let TemplateKind::Table(tt) = &mut t.kind {
            tt.insert_table(0, 2, 3, ColWidth::Fixed(30.0), A4.0, 20.0).unwrap();
        }
        t.tags = vec!["教育学习".into()];
        t
    }

    #[test]
    fn save_scan_remove_and_sync() {
        let (l, _) = lib("basic");
        l.save(&table()).unwrap();
        l.save(&Template::ruled("信纸: 22/行", A4, (20.0, 20.0, 20.0, 20.0), 22)).unwrap();
        assert!(l.default_dir.join("信纸_ 22_行.svg").exists(), "unsafe characters replaced");
        let names: Vec<String> = l.scan().into_iter().map(|e| e.template.name).collect();
        assert!(names.contains(&"体温表".into()) && names.contains(&"信纸: 22/行".into()));
        // A template in an extra folder is read-only and not duplicated.
        std::fs::create_dir_all(&l.extra_dirs[0]).unwrap();
        std::fs::write(l.extra_dirs[0].join("shared.svg"), crate::svg::write(&Template::blank("共享", A4), &l.assets)).unwrap();
        std::fs::write(l.extra_dirs[0].join("dup.svg"), crate::svg::write(&table(), &l.assets)).unwrap();
        let all = l.scan();
        assert_eq!(all.len(), 3);
        assert!(!all.iter().find(|e| e.template.name == "共享").unwrap().editable);
        // sync: drop 信纸, keep the shared one.
        let store = TemplateStore { templates: all.iter().map(|e| e.template.clone()).filter(|t| t.name != "信纸: 22/行").collect() };
        l.sync(&store).unwrap();
        assert_eq!(l.scan().len(), 2);
        assert!(l.extra_dirs[0].join("shared.svg").exists());
        assert!(!l.remove("共享").unwrap(), "extra folders are never written");
        assert!(l.remove("体温表").unwrap());
    }

    #[test]
    fn zip_svg_and_xlsx_round_trips() {
        let (l, root) = lib("export");
        let (a, b) = (table(), Template::grid("作文", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9)));
        let zip = root.join("all.zip");
        l.export(&[a.clone(), b.clone()], &zip).unwrap();
        let xlsx = root.join("form.xlsx");
        l.export(std::slice::from_ref(&a), &xlsx).unwrap();
        assert!(l.export(std::slice::from_ref(&b), &root.join("grid.xlsx")).is_err(), "only tables go to Excel");
        let (l2, _) = lib("import");
        let names = l2.import(&[zip.clone(), zip, xlsx]).unwrap();
        assert_eq!(names, vec!["体温表", "作文", "体温表 (2)", "作文 (2)", "form"]);
        let back: Vec<Template> = l2.scan().into_iter().map(|e| e.template).collect();
        assert_eq!(back.iter().find(|t| t.name == "体温表").unwrap(), &a);
        let TemplateKind::Table(x) = &back.iter().find(|t| t.name == "form").unwrap().kind else { panic!("table") };
        assert_eq!(x.pages[0].tables[0].cells.len(), 6);
    }

    #[test]
    fn migrates_templates_toml_once_and_moves() {
        let (l, root) = lib("migrate");
        let old = root.join("templates.toml");
        let mut s = TemplateStore::builtins();
        s.upsert(table());
        s.save(&old).unwrap();
        let store = l.load_store(&old).unwrap();
        assert_eq!(store.templates.len(), 4);
        assert!(!old.exists() && root.join("templates.toml.bak").exists());
        assert_eq!(l.load_store(&old).unwrap().templates.len(), 4, "second start: nothing to migrate");
        let moved = l.move_default(&root.join("Moved")).unwrap();
        assert_eq!(moved.scan().len(), 4);
        assert!(svg_files(&l.default_dir).is_empty());
        // An empty library gets the built-ins.
        let (fresh, root2) = lib("fresh");
        assert_eq!(fresh.load_store(&root2.join("none.toml")).unwrap().templates.len(), 3);
    }
}
