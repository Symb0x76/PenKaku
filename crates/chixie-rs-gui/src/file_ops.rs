//! Documents on disk: DOCX (text + per-character ids + handwriting seed in a custom XML part, so a
//! reopened document keeps exactly the same handwriting), `.chx` v1 and plain `.txt` import, the
//! autosaved draft, and G-code export (one file per page).

use std::path::{Path, PathBuf};

use chixie_rs_core::profile::DeviceProfile;
use chixie_rs_output as gcode;
use chixie_rs_output::Written;
use chixie_rs_doc::EditDoc;
use chixie_rs_doc::docx::TableShape;

pub struct Loaded {
    pub doc: EditDoc,
    /// Handwriting seed stored with our own documents; `None` for `.txt` and foreign `.docx`.
    pub seed: Option<u64>,
}

/// Save as DOCX (atomic).
pub fn save(path: &Path, doc: &EditDoc, seed: u64, tables: &[Vec<TableShape>]) -> std::io::Result<()> {
    chixie_rs_doc::save(path, doc.document(), seed, tables)
}

/// Word table shapes of a table form template's pages (empty for other templates).
pub fn table_shapes(t: Option<&chixie_rs_template::Template>) -> Vec<Vec<TableShape>> {
    let Some(chixie_rs_template::TemplateKind::Table(tt)) = t.map(|t| &t.kind) else { return Vec::new() };
    tt.pages
        .iter()
        .map(|p| {
            p.tables
                .iter()
                .map(|t| TableShape {
                    cols: t.cols.clone(),
                    rows: t.rows.len() as u32,
                    cells: t.reading_order().into_iter().map(|i| t.cells[i].span).map(|s| (s.r0, s.c0, s.r1, s.c1)).collect(),
                })
                .collect()
        })
        .collect()
}

/// Open `.docx`, `.chx` (migrated) or anything else as UTF-8 text.
pub fn open(path: &Path) -> std::io::Result<Loaded> {
    let l = chixie_rs_doc::open(path)?;
    Ok(Loaded { doc: EditDoc::from_document(l.doc), seed: l.seed })
}

pub fn draft_path(settings_path: &Path) -> PathBuf {
    settings_path.parent().unwrap_or_else(|| Path::new(".")).join("draft.docx")
}

/// The autosaved draft; falls back to the pre-DOCX `draft.chx`.
pub fn open_draft(settings_path: &Path) -> std::io::Result<Loaded> {
    let p = draft_path(settings_path);
    open(&p).or_else(|_| open(&p.with_extension("chx")))
}

pub fn documents_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(|| PathBuf::from("."))
}

/// Write one G-code file per page (pages as written, see `chixie_rs_output::plan`): `name.gcode` for a single page, `name_p1.gcode`, … otherwise
/// (each page is a separate sheet). Returns the written paths.
pub fn export_files(pages: &[Written], profile: &DeviceProfile, path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("chixie").to_string();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let p = if pages.len() == 1 { path.to_path_buf() } else { dir.join(format!("{stem}_p{}.gcode", i + 1)) };
        std::fs::write(&p, gcode::emit_written(page, profile).text)?;
        out.push(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join("chixie-rs-gui-test-docs");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn docx_round_trip_keeps_ids_and_seed() {
        let p = dir().join("a.docx");
        let mut d = EditDoc::from_text("你好");
        d.move_to(0, false);
        d.insert("啊"); // id 2 at the front: ids are no longer 0..n
        save(&p, &d, 42, &[]).unwrap();
        let l = open(&p).unwrap();
        assert_eq!(l.doc.text(), "啊你好");
        assert_eq!(l.doc.ids(), d.ids());
        assert_eq!(l.seed, Some(42));
    }

    #[test]
    fn legacy_draft_is_still_found() {
        let settings = dir().join("legacy").join("settings.toml");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(draft_path(&settings));
        std::fs::write(draft_path(&settings).with_extension("chx"), br#"{"version":1,"text":"ab","ids":[3,1],"seed":9}"#).unwrap();
        let l = open_draft(&settings).unwrap();
        assert_eq!((l.doc.ids(), l.seed), (&[3u64, 1][..], Some(9)));
    }

    #[test]
    fn txt_opens_with_fresh_ids() {
        let p = dir().join("b.txt");
        std::fs::write(&p, "\u{feff}a\r\nb").unwrap();
        let l = open(&p).unwrap();
        assert_eq!(l.doc.text(), "a\nb");
        assert_eq!(l.seed, None);
    }
}
