//! Document model (plan §2.2): characters with stable ids and run attributes, paragraphs,
//! undoable commands, and the DOCX file format (with `.chx` v1 migration).

pub mod chx;
pub mod docx;
pub mod edit;
pub mod model;

use std::path::Path;

pub use docx::Loaded;
pub use edit::EditDoc;
pub use model::{Align, Attrs, Char, Command, Document, Para};

/// Open a document by extension: `.docx`, `.chx` (v1, migrated), anything else as UTF-8 text
/// (BOM and CRLF tolerated; fresh ids, no seed).
pub fn open(path: &Path) -> std::io::Result<Loaded> {
    let bytes = std::fs::read(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let invalid = |e: String| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    match ext.as_deref() {
        Some("docx") => docx::read(&bytes).map_err(|e| invalid(e.to_string())),
        Some("chx") => chx::read(&bytes).map_err(|e| invalid(e.to_string())),
        _ => {
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            Ok(Loaded { doc: Document::from_text(text.trim_start_matches('\u{feff}')), seed: None })
        }
    }
}

/// Save as DOCX. Write-then-rename, so a crash never leaves a half-written document. `tables`:
/// the table form's pages, when the document is written on one (see [`docx::write_tables`]).
pub fn save(path: &Path, doc: &Document, seed: u64, tables: &[Vec<docx::TableShape>]) -> std::io::Result<()> {
    let bytes = docx::write_tables(doc, seed, tables).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("docx.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
