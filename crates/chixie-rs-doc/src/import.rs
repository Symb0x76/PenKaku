//! 导入 into the current document: text from a file, added by 覆盖 / 追加 / 在光标处插入 as one
//! undo step. Text files are decoded like ChiXie3 reads them (UTF-8 / UTF-16 with a BOM, UTF-8,
//! else GB18030); DOCX keeps its character formats. PDF and spreadsheet text comes from the
//! callers (`chixie-rs-import`, `chixie-rs-template`) as plain lines.

use crate::model::Attrs;
use crate::{EditDoc, docx};

/// Where imported text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// 覆盖: replace everything.
    Overwrite,
    /// 追加: after the last character, on a new paragraph.
    Append,
    /// 在光标处插入: over the selection / at the caret.
    AtCaret,
}

/// Decode a text file: a BOM picks UTF-8 / UTF-16; else UTF-8 when valid, else GB18030 (what
/// Chinese Windows Notepad writes as "ANSI"). CRLF / CR become LF.
pub fn decode_text(bytes: &[u8]) -> String {
    let text = if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        enc.decode_without_bom_handling(&bytes[bom..]).0.into_owned()
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => encoding_rs::GB18030.decode_without_bom_handling(bytes).0.into_owned(),
        }
    };
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// A DOCX's text with each char's formatting.
pub fn docx_chars(bytes: &[u8]) -> Result<Vec<(char, Attrs)>, String> {
    let l = docx::read(bytes).map_err(|e| e.to_string())?;
    let d = l.doc;
    Ok(d.chars().iter().enumerate().map(|(i, &c)| (c, d.attrs(i).clone())).collect())
}

impl EditDoc {
    /// Add imported chars by `mode` (one undo step). Chars without their own formatting take
    /// the formatting at the insertion point.
    pub fn import_chars(&mut self, chars: Vec<(char, Option<Attrs>)>, mode: ImportMode) {
        let len = self.len();
        let (range, sep) = match mode {
            ImportMode::Overwrite => (0..len, false),
            ImportMode::Append => (len..len, len > 0 && self.chars().last() != Some(&'\n')),
            ImportMode::AtCaret => (self.selection(), false),
        };
        let base = self.document().attrs_at(range.start);
        let mut text: String = if sep { "\n".into() } else { String::new() };
        let mut attrs: Vec<Attrs> = if sep { vec![base.clone()] } else { Vec::new() };
        for (c, a) in chars {
            text.push(c);
            attrs.push(a.unwrap_or_else(|| base.clone()));
        }
        self.select_range(range);
        self.insert_with_attrs(&text, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;

    fn plain(s: &str) -> Vec<(char, Option<Attrs>)> {
        s.chars().map(|c| (c, None)).collect()
    }

    #[test]
    fn text_encodings() {
        assert_eq!(decode_text("a\r\nb".as_bytes()), "a\nb");
        assert_eq!(decode_text(b"\xEF\xBB\xBF\xE4\xBD\xA0"), "你");
        // UTF-16 LE with BOM.
        assert_eq!(decode_text(&[0xFF, 0xFE, 0x60, 0x4F, 0x7D, 0x59]), "你好");
        // GB18030 / GBK "你好" (C4E3 BAC3).
        assert_eq!(decode_text(&[0xC4, 0xE3, 0xBA, 0xC3]), "你好");
    }

    #[test]
    fn overwrite_append_and_insert() {
        let mut d = EditDoc::from_text("旧文");
        d.import_chars(plain("新"), ImportMode::Append);
        assert_eq!(d.text(), "旧文\n新");
        d.select_range(1..1);
        d.import_chars(plain("插"), ImportMode::AtCaret);
        assert_eq!(d.text(), "旧插文\n新");
        d.import_chars(plain("全新"), ImportMode::Overwrite);
        assert_eq!(d.text(), "全新");
        // Each import is one undo step.
        assert!(d.undo());
        assert_eq!(d.text(), "旧插文\n新");
        // Appending to an empty document adds no blank line.
        let mut e = EditDoc::from_text("");
        e.import_chars(plain("a"), ImportMode::Append);
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn docx_import_keeps_formats() {
        let mut src = Document::from_text("下划");
        src.apply(crate::Command::SetAttrs { at: 0, attrs: vec![Attrs { underline: true, ..Attrs::default() }] });
        let bytes = docx::write(&src, 1).unwrap();
        let chars = docx_chars(&bytes).unwrap();
        let mut d = EditDoc::from_text("");
        d.import_chars(chars.into_iter().map(|(c, a)| (c, Some(a))).collect(), ImportMode::Overwrite);
        assert_eq!(d.text(), "下划");
        assert!(d.document().attrs(0).underline && !d.document().attrs(1).underline);
    }
}
