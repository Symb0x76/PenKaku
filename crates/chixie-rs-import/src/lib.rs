//! Table frames from PDFs, scans and photos, detected locally (ChiXie3 sends them to a cloud
//! service instead). Two paths meet in [`grid`]:
//! - vector PDFs: the page's line and rectangle drawing operators (`pdf`);
//! - images and scanned PDFs: threshold, perspective fix, line masks (`raster`).
//!
//! The result is plain geometry; the GUI turns it into a table template.

pub mod grid;
pub mod pdf;
#[doc(hidden)]
pub mod synth;

pub use grid::{DetectedTable, HSeg, VSeg};
