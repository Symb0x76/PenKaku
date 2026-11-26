//! Import ChiXie3 page templates (`%USERPROFILE%\ChiXie3\conf\note_template.cx` and
//! `composition_template.cx`).
//!
//! The files are Python pickles (protocol 4) of `{name: NoteTemplate | CompositionTemplate}`.
//! The safe pickle reader lives in `penkaku_core::pickle`: class names are read, never run.
//! Field meanings: docs/protocol.md (templates).

use crate::template::{Template, TemplateKind};
use penkaku_core::pickle::{self, Value};
use penkaku_core::{Error, Result};

/// Read every template from one `.cx` file's bytes.
pub fn import(bytes: &[u8]) -> Result<Vec<Template>> {
    let Value::Dict(items) = pickle::parse(bytes)? else { return Err(bad("top level is not a dict")) };
    let mut out = Vec::new();
    for (k, v) in &items {
        let name = k.as_str().ok_or_else(|| bad("template key is not a string"))?;
        let Value::Object { class, state } = v else { return Err(bad("template is not an object")) };
        let f = Fields(state.as_ref());
        let t = match class.rsplit('.').next().unwrap_or_default() {
            "NoteTemplate" => note(name, &f)?,
            "CompositionTemplate" => composition(name, &f)?,
            other => return Err(bad(&format!("unknown template class {other}"))),
        };
        out.push(t);
    }
    Ok(out)
}

fn note(name: &str, f: &Fields) -> Result<Template> {
    let paper = (f.num("page_width")?, f.num("page_height")?);
    let margins = (f.num("margin_left")?, f.num("margin_top")?, f.num("margin_right")?, f.num("margin_bottom")?);
    let (lo, hi) = (f.num("min_row_num")? as u32, f.num("max_row_num")? as u32);
    let dual = f.bool("enable_dual_line_mode").unwrap_or(false);
    let kind = TemplateKind::Ruled {
        rows_min: lo.min(hi),
        rows_max: hi.max(lo),
        dual_line: dual.then(|| f.num("up_row_height").unwrap_or(2.5)),
        dashed: f.bool("enable_dashed_line_partition").unwrap_or(true),
        columns: f.num("column_num").map(|n| n as u32).unwrap_or(1).max(1),
        // Equal columns only; ChiXie3 keeps per-column gaps in `column_space`.
        column_gap: f.first_num("column_space").unwrap_or(6.0),
        column_widths: Vec::new(),
        divider: f.bool("show_dividing_line").unwrap_or(true),
        write_upper: false,
    };
    Ok(Template { name: name.into(), paper, margins, kind, tags: Vec::new(), favourite: false })
}

fn composition(name: &str, f: &Fields) -> Result<Template> {
    Ok(Template::grid(
        name,
        (f.num("page_width")?, f.num("page_height")?),
        f.num("left_margin")?,
        f.num("top_margin")?,
        f.num("cell_row_number")? as u32,
        f.num("cell_column_number")? as u32,
        (f.num("cell_width")?, f.num("cell_height")?),
        (f.num("column_spacing").unwrap_or(0.0), f.num("row_spacing").unwrap_or(0.0)),
    ))
}

fn bad(msg: &str) -> Error {
    Error::Config(format!("ChiXie3 template file: {msg}"))
}

/// An object's `__dict__` state.
struct Fields<'a>(&'a Value);

impl Fields<'_> {
    fn num(&self, key: &str) -> Result<f32> {
        match self.0.get(key) {
            Some(Value::Int(v)) => Ok(*v as f32),
            Some(Value::Float(v)) => Ok(*v as f32),
            _ => Err(bad(&format!("missing number {key}"))),
        }
    }
    fn bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Int(v)) => Some(*v != 0),
            _ => None,
        }
    }
    fn first_num(&self, key: &str) -> Option<f32> {
        let Some(Value::List(xs)) = self.0.get(key) else { return None };
        match xs.first() {
            Some(Value::Int(v)) => Some(*v as f32),
            Some(Value::Float(v)) => Some(*v as f32),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../penkaku-core/tests/fixtures").join(name)).unwrap()
    }

    #[test]
    fn imports_note_templates() {
        let ts = import(&fixture("note_template.cx")).unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0], Template::ruled("信纸22行", (160.0, 219.8), (1.5, 1.0, 1.5, 1.0), 22));
        let t = &ts[1];
        assert_eq!((t.name.as_str(), t.paper, t.margins), ("随机双栏", (210.0, 297.0), (20.0, 30.0, 20.0, 20.0)));
        assert_eq!(
            t.kind,
            TemplateKind::Ruled { rows_min: 20, rows_max: 26, dual_line: Some(2.5), dashed: true, columns: 2, column_gap: 8.0, column_widths: vec![], divider: true, write_upper: false }
        );
    }

    #[test]
    fn imports_composition_grid() {
        let ts = import(&fixture("composition_template.cx")).unwrap();
        assert_eq!(ts, vec![Template::grid("作文格", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9))]);
    }

    /// Reads the real ChiXie3 files of this machine's user: `cargo test -- --ignored real_files`.
    #[test]
    #[ignore]
    fn real_files() {
        let dir = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join("ChiXie3/conf");
        for f in ["note_template.cx", "composition_template.cx"] {
            let ts = import(&std::fs::read(dir.join(f)).unwrap()).unwrap();
            assert!(!ts.is_empty());
            eprintln!("{f}: {ts:#?}");
        }
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(import(b"not a pickle").is_err());
        assert!(import(&[0x80, 0x04, 0x95]).is_err());
        assert!(import(b"N.").is_err()); // valid pickle, wrong shape
    }
}
