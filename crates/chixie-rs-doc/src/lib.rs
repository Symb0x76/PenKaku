//! Document model (plan §2.2): characters with stable ids and run attributes, paragraphs,
//! undoable commands, and the DOCX file format (with `.chx` v1 migration).

pub mod edit;
pub mod model;

pub use edit::EditDoc;
pub use model::{Align, Attrs, Char, Command, Document, Para};
