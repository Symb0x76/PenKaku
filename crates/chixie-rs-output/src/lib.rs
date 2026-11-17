//! Output pipeline (plan §2.3): laid-out pages → ordered, transformed strokes → G-code with a
//! resume map. Settings live in `chixie_rs_core::{profile, config}`; this crate only computes.

pub mod emit;
pub mod plan;

pub use emit::{PageCode, emit, emit_document, emit_mapped, emit_written, standalone};
pub use plan::{OutputOptions, Sheet, Written, plan, plan_all, write};
