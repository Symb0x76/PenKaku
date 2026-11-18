//! Page templates (ChiXie3 模板管理): note paper, composition grids and tables, their store, and
//! the reader for ChiXie3's own `.cx` note / grid templates.

pub mod chixie3_templates;
pub mod fill;
pub mod svg;
pub mod table;
pub mod template;
pub mod xlsx;

pub use table::TableTemplate;
pub use template::{Template, TemplateError, TemplateKind, TemplateStore};
