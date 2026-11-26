//! Document model (plan §2.2): characters with stable ids and run attributes, paragraphs,
//! undoable commands, and the DOCX file format (with `.chx` v1 migration).

pub mod chx;
pub mod docx;
pub mod edit;
pub mod find;
pub mod import;
pub mod model;

use std::path::Path;

pub use docx::Loaded;
pub use edit::EditDoc;
pub use find::FindOpts;
pub use model::{Align, Attrs, Char, Command, Document, Para};

/// Open a document by extension: `.docx`, `.chx` (v1, migrated), anything else as text
/// (UTF-8 / UTF-16 / GB18030, see [`import::decode_text`]; fresh ids, no seed).
pub fn open(path: &Path) -> std::io::Result<Loaded> {
    let bytes = std::fs::read(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let invalid = |e: String| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    match ext.as_deref() {
        Some("docx") => docx::read(&bytes).map_err(|e| invalid(e.to_string())),
        Some("chx") => chx::read(&bytes).map_err(|e| invalid(e.to_string())),
        _ => Ok(Loaded { doc: Document::from_text(&import::decode_text(&bytes)), seed: None }),
    }
}

/// How many pictures (图k spaces) the document holds.
pub fn picture_count(doc: &Document) -> usize {
    (0..doc.len()).filter(|&i| doc.chars()[i] == model::OBJECT && matches!(doc.attrs(i).block, Some(model::Block::Picture { .. }))).count()
}

/// The image bytes of the picture whose anchor is char `i`.
pub fn picture_data(doc: &Document, i: usize) -> Option<std::sync::Arc<[u8]>> {
    match doc.attrs(i.min(doc.len().saturating_sub(1))).block.as_ref() {
        Some(model::Block::Picture { data, .. }) => data.0.clone(),
        _ => None,
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
