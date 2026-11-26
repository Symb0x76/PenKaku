//! `.chx` v1 (penkaku before M2): JSON `{version, text, ids, seed}`. Read only; documents are
//! saved as DOCX now.

use serde::Deserialize;

use crate::docx::Loaded;
use crate::model::Document;

#[derive(Deserialize)]
struct ChxV1 {
    #[allow(dead_code)]
    version: u32,
    text: String,
    ids: Vec<u64>,
    seed: u64,
}

pub fn read(bytes: &[u8]) -> Result<Loaded, serde_json::Error> {
    let f: ChxV1 = serde_json::from_slice(bytes)?;
    Ok(Loaded { doc: Document::from_parts(f.text.chars().collect(), f.ids), seed: Some(f.seed) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_fixtures_open_identically() {
        // Written by penkaku 0.1 (`file_ops::save_chx`): ids are not 0..n after an edit.
        let l = read(include_bytes!("../tests/fixtures/v1-edited.chx")).unwrap();
        assert_eq!(l.doc.text(), "啊你好\n赤写");
        assert_eq!(l.doc.ids(), &[5, 0, 1, 2, 3, 4]);
        assert_eq!(l.seed, Some(42));
        let l = read(include_bytes!("../tests/fixtures/v1-plain.chx")).unwrap();
        assert_eq!(l.doc.text(), "你好，chixie-rs！\n春眠不觉晓，处处闻啼鸟。");
        assert_eq!(l.doc.ids(), &(0..l.doc.len() as u64).collect::<Vec<_>>()[..]);
        assert_eq!(l.seed, Some(20251107));
    }

    #[test]
    fn v1_saves_as_docx_unchanged() {
        let l = read(include_bytes!("../tests/fixtures/v1-edited.chx")).unwrap();
        let back = crate::docx::read(&crate::docx::write(&l.doc, l.seed.unwrap()).unwrap()).unwrap();
        assert!(back.doc.same_content(&l.doc));
        assert_eq!(back.seed, l.seed);
    }

    #[test]
    fn mismatched_ids_are_renumbered() {
        let l = read(br#"{"version":1,"text":"ab","ids":[9],"seed":1}"#).unwrap();
        assert_eq!(l.doc.ids(), &[0, 1]);
    }
}
