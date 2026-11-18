//! Page templates (ChiXie3 模板管理): note paper, composition grids and tables, their store, and
//! the reader for ChiXie3's own `.cx` note / grid templates.

pub mod chixie3_templates;
pub mod template;

pub use template::{Template, TemplateKind, TemplateStore};
