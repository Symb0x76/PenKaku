//! DOCX, the document format. A small OOXML writer/reader of our own (zip + quick-xml):
//!
//! - `word/document.xml`: one `w:p` per paragraph (alignment, first-line indent) with runs split
//!   where the formatting changes (`rFonts`, `sz`, `color`, `position`), so Word, WPS and
//!   LibreOffice show the text, fonts and colours.
//! - `customXml/item1.xml` (namespace [`NS`]): what Word can't hold exactly — the handwriting seed,
//!   character ids (run-length encoded), exact attributes and paragraph records, plus the text they
//!   belong to. Word keeps custom XML parts when it saves.
//!
//! Reading our own file gives back the exact document. If the text was edited elsewhere, the
//! unchanged prefix and suffix keep their ids and formatting and the changed middle gets fresh
//! ids. A foreign DOCX opens with fresh ids and no seed.

use std::io::{Cursor, Read as _, Write as _};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::model::{Align, Attrs, Char, Document, Para};

/// Namespace of our custom XML part.
pub const NS: &str = "urn:chixie-rs:doc:1";
const MM_PER_PT: f32 = 25.4 / 72.0;
const TWIPS_PER_MM: f32 = 1440.0 / 25.4;

/// A document read from a file.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub doc: Document,
    /// Handwriting seed saved with the document (`None` for foreign files and plain text).
    pub seed: Option<u64>,
}

#[derive(Debug)]
pub enum Error {
    Zip(zip::result::ZipError),
    Xml(String),
    Io(std::io::Error),
    /// Not a Word document (no `word/document.xml`).
    NotDocx,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "zip: {e}"),
            Error::Xml(e) => write!(f, "xml: {e}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::NotDocx => f.write_str("不是 Word 文档（缺少 word/document.xml）"),
        }
    }
}

impl std::error::Error for Error {}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Zip(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::Xml(e.to_string())
    }
}
impl From<quick_xml::encoding::EncodingError> for Error {
    fn from(e: quick_xml::encoding::EncodingError) -> Self {
        Error::Xml(e.to_string())
    }
}
impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        Error::Xml(e.to_string())
    }
}

/// The custom part's payload (JSON inside `<data>`).
#[derive(Debug, Serialize, Deserialize)]
struct Extras {
    version: u32,
    seed: u64,
    /// The text the ids belong to; compared on load to detect edits made elsewhere.
    text: String,
    /// Ids as runs of consecutive values: (first id, count).
    ids: Vec<(u64, u32)>,
    styles: Vec<Attrs>,
    /// Style index per character as runs: (style, count).
    attrs: Vec<(u32, u32)>,
    paras: Vec<Para>,
}

fn rle_ids(ids: &[u64]) -> Vec<(u64, u32)> {
    let mut out: Vec<(u64, u32)> = Vec::new();
    for &id in ids {
        match out.last_mut() {
            Some((first, n)) if *first + u64::from(*n) == id => *n += 1,
            _ => out.push((id, 1)),
        }
    }
    out
}

fn rle<T: PartialEq + Copy>(v: impl IntoIterator<Item = T>) -> Vec<(T, u32)> {
    let mut out: Vec<(T, u32)> = Vec::new();
    for x in v {
        match out.last_mut() {
            Some((y, n)) if *y == x => *n += 1,
            _ => out.push((x, 1)),
        }
    }
    out
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

// ---- writing ----

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/></Relationships>"#;

const ITEM_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#;

const ITEM_PROPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<ds:datastoreItem ds:itemID="{6C1F1E43-2B7A-4E0B-9C6E-6368697869E5}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"><ds:schemaRefs><ds:schemaRef ds:uri="urn:chixie-rs:doc:1"/></ds:schemaRefs></ds:datastoreItem>"#;

fn run_props(a: &Attrs) -> String {
    let mut p = String::new();
    if let Some(f) = &a.font {
        let f = esc(f);
        p += &format!(r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/>"#);
    }
    if let Some(c) = a.colour {
        p += &format!(r#"<w:color w:val="{:02X}{:02X}{:02X}"/>"#, c[0], c[1], c[2]);
    }
    if let Some(s) = a.size_pt {
        let hp = (s * 2.0).round().max(1.0) as i64;
        p += &format!(r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#);
    }
    if let Some(v) = a.v_offset {
        p += &format!(r#"<w:position w:val="{}"/>"#, (v / MM_PER_PT * 2.0).round() as i64);
    }
    if p.is_empty() { p } else { format!("<w:rPr>{p}</w:rPr>") }
}

fn para_props(p: &Para) -> String {
    let mut s = String::new();
    let jc = match p.align {
        Align::Left => None,
        Align::Center => Some("center"),
        Align::Right => Some("right"),
        Align::Justify => Some("both"),
    };
    if let Some(i) = p.first_indent {
        s += &format!(r#"<w:ind w:firstLine="{}"/>"#, (i * TWIPS_PER_MM).round() as i64);
    }
    if let Some(j) = jc {
        s += &format!(r#"<w:jc w:val="{j}"/>"#);
    }
    if s.is_empty() { s } else { format!("<w:pPr>{s}</w:pPr>") }
}

fn document_xml(doc: &Document) -> String {
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    let chars = doc.chars();
    let mut para = 0;
    let mut i = 0;
    loop {
        x += "<w:p>";
        x += &para_props(&doc.paras()[para]);
        // Runs up to the paragraph's end.
        while i < chars.len() && chars[i] != '\n' {
            let a = doc.attrs(i);
            let start = i;
            while i < chars.len() && chars[i] != '\n' && doc.attrs(i) == a {
                i += 1;
            }
            x += "<w:r>";
            x += &run_props(a);
            for (k, piece) in chars[start..i].split(|&c| c == '\t').enumerate() {
                if k > 0 {
                    x += "<w:tab/>";
                }
                if !piece.is_empty() {
                    x += &format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(&piece.iter().collect::<String>()));
                }
            }
            x += "</w:r>";
        }
        x += "</w:p>";
        if i >= chars.len() {
            break;
        }
        i += 1; // the '\n'
        para += 1;
    }
    x += "</w:body></w:document>";
    x
}

fn extras_xml(doc: &Document, seed: u64) -> String {
    // Style table local to this file: first-seen order.
    let mut styles: Vec<Attrs> = Vec::new();
    let mut ix = Vec::with_capacity(doc.len());
    for i in 0..doc.len() {
        let a = doc.attrs(i);
        let k = styles.iter().position(|s| s == a).unwrap_or_else(|| {
            styles.push(a.clone());
            styles.len() - 1
        });
        ix.push(k as u32);
    }
    let e = Extras { version: 1, seed, text: doc.text(), ids: rle_ids(doc.ids()), styles, attrs: rle(ix), paras: doc.paras().to_vec() };
    let json = serde_json::to_string(&e).expect("extras serialise");
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<chixie xmlns="{NS}"><data>{}</data></chixie>"#, esc(&json))
}

/// The document as DOCX bytes. Deterministic: the same document and seed give the same bytes.
pub fn write(doc: &Document, seed: u64) -> Result<Vec<u8>, Error> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).last_modified_time(DateTime::default());
    let parts: [(&str, String); 7] = [
        ("[Content_Types].xml", CONTENT_TYPES.into()),
        ("_rels/.rels", ROOT_RELS.into()),
        ("word/document.xml", document_xml(doc)),
        ("word/_rels/document.xml.rels", DOC_RELS.into()),
        ("customXml/item1.xml", extras_xml(doc, seed)),
        ("customXml/itemProps1.xml", ITEM_PROPS.into()),
        ("customXml/_rels/item1.xml.rels", ITEM_RELS.into()),
    ];
    for (name, body) in parts {
        z.start_file(name, opt)?;
        z.write_all(body.as_bytes())?;
    }
    Ok(z.finish()?.into_inner())
}

// ---- reading ----

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>, Error> {
    Ok(match e.try_get_attribute(name)? {
        Some(a) => Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0)?.into_owned()),
        None => None,
    })
}

fn predefined(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// Characters with their Word formatting, and the paragraph records.
type Parsed = (Vec<(char, Attrs)>, Vec<Para>);

/// Text, per-character formatting and paragraphs of `word/document.xml`.
fn parse_document(xml: &str) -> Result<Parsed, Error> {
    let mut r = Reader::from_str(xml);
    let mut chars: Vec<(char, Attrs)> = Vec::new();
    let mut paras: Vec<Para> = Vec::new();
    let (mut in_ppr, mut in_rpr, mut in_t) = (false, false, false);
    let mut run = Attrs::default();
    loop {
        let ev = r.read_event()?;
        let (e, open) = match &ev {
            Event::Start(e) => (Some(e.clone()), true),
            Event::Empty(e) => (Some(e.clone()), false),
            _ => (None, false),
        };
        if let Some(e) = e {
            match e.name().as_ref() {
                b"w:p" => {
                    if !paras.is_empty() {
                        chars.push(('\n', Attrs::default()));
                    }
                    paras.push(Para::default());
                }
                b"w:pPr" => in_ppr = open,
                b"w:rPr" if !in_ppr => in_rpr = open,
                b"w:r" => run = Attrs::default(),
                b"w:t" => in_t = open,
                b"w:tab" if !in_ppr => chars.push(('\t', run.clone())),
                b"w:br" | b"w:cr" => {
                    chars.push(('\n', run.clone()));
                    let p = paras.last().cloned().unwrap_or_default();
                    paras.push(p);
                }
                b"w:jc" if in_ppr => {
                    if let Some(p) = paras.last_mut() {
                        p.align = match attr(&e, "w:val")?.as_deref() {
                            Some("center") => Align::Center,
                            Some("right" | "end") => Align::Right,
                            Some("both" | "distribute") => Align::Justify,
                            _ => Align::Left,
                        };
                    }
                }
                b"w:ind" if in_ppr => {
                    if let (Some(p), Some(v)) = (paras.last_mut(), attr(&e, "w:firstLine")?.and_then(|v| v.parse::<f32>().ok())) {
                        p.first_indent = Some(v / TWIPS_PER_MM);
                    }
                }
                b"w:rFonts" if in_rpr => {
                    run.font = match attr(&e, "w:eastAsia")? {
                        Some(f) => Some(f),
                        None => attr(&e, "w:ascii")?,
                    };
                }
                b"w:color" if in_rpr => {
                    run.colour = attr(&e, "w:val")?.filter(|v| v.len() == 6).and_then(|v| {
                        let b = |k: usize| u8::from_str_radix(&v[k..k + 2], 16).ok();
                        Some([b(0)?, b(2)?, b(4)?])
                    });
                }
                b"w:sz" if in_rpr => run.size_pt = attr(&e, "w:val")?.and_then(|v| v.parse::<f32>().ok()).map(|hp| hp / 2.0),
                b"w:position" if in_rpr => {
                    run.v_offset = attr(&e, "w:val")?.and_then(|v| v.parse::<f32>().ok()).map(|hp| hp / 2.0 * MM_PER_PT);
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::End(e) => match e.name().as_ref() {
                b"w:pPr" => in_ppr = false,
                b"w:rPr" => in_rpr = false,
                b"w:t" => in_t = false,
                _ => {}
            },
            Event::Text(t) if in_t => chars.extend(t.decode()?.chars().map(|c| (c, run.clone()))),
            Event::GeneralRef(g) if in_t => {
                let c = match g.resolve_char_ref()? {
                    Some(c) => Some(c),
                    None => predefined(&g.decode()?),
                };
                chars.extend(c.map(|c| (c, run.clone())));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if paras.is_empty() {
        paras.push(Para::default());
    }
    Ok((chars, paras))
}

/// Our extras, from whichever `customXml/item*.xml` carries [`NS`] (Word may renumber them).
fn find_extras(zip: &mut ZipArchive<Cursor<&[u8]>>) -> Option<Extras> {
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).filter(|n| n.starts_with("customXml/item") && !n.contains("Props")).map(|n| n.into_owned()).collect();
    for n in names {
        let mut s = String::new();
        if zip.by_name(&n).ok()?.read_to_string(&mut s).is_err() || !s.contains(NS) {
            continue;
        }
        let mut r = Reader::from_str(&s);
        let mut in_data = false;
        let mut json = String::new();
        loop {
            match r.read_event() {
                Ok(Event::Start(e)) if e.local_name().as_ref() == b"data" => in_data = true,
                Ok(Event::End(e)) if e.local_name().as_ref() == b"data" => break,
                Ok(Event::Text(t)) if in_data => json += &t.decode().ok()?,
                Ok(Event::GeneralRef(g)) if in_data => json.push(g.resolve_char_ref().ok().flatten().or_else(|| predefined(&g.decode().ok()?))?),
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        if let Ok(e) = serde_json::from_str::<Extras>(&json) {
            return Some(e);
        }
    }
    None
}

/// Rebuild with our extras: exact when the text is unchanged; otherwise the common prefix and
/// suffix keep their ids and formatting, the middle gets fresh ids and Word's formatting.
fn merge(parsed: Vec<(char, Attrs)>, paras: Vec<Para>, ex: &Extras) -> Option<Document> {
    let ids: Vec<u64> = ex.ids.iter().flat_map(|&(f, n)| (0..u64::from(n)).map(move |k| f + k)).collect();
    let styles: Vec<usize> = ex.attrs.iter().flat_map(|&(s, n)| std::iter::repeat_n(s as usize, n as usize)).collect();
    let old: Vec<char> = ex.text.chars().collect();
    if ids.len() != old.len() || styles.len() != old.len() || styles.iter().any(|&s| s >= ex.styles.len()) {
        return None;
    }
    let new: Vec<char> = parsed.iter().map(|(c, _)| *c).collect();
    let exact = new == old;
    let pre = new.iter().zip(&old).take_while(|(a, b)| a == b).count();
    let max_suf = new.len().min(old.len()) - pre;
    let suf = new.iter().rev().zip(old.iter().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
    let mut next = ids.iter().max().map_or(0, |m| m + 1);
    let chars = parsed
        .into_iter()
        .enumerate()
        .map(|(i, (ch, a))| {
            let from_old = if i < pre {
                Some(i)
            } else if i >= new.len() - suf {
                Some(old.len() - (new.len() - i))
            } else {
                None
            };
            match from_old {
                Some(j) => Char { ch, id: ids[j], attrs: ex.styles[styles[j]].clone() },
                None => {
                    next += 1;
                    Char { ch, id: next - 1, attrs: a }
                }
            }
        })
        .collect();
    let paras = if exact && ex.paras.len() == paras.len() { ex.paras.clone() } else { paras };
    Document::from_loaded(chars, paras)
}

/// Read DOCX bytes.
pub fn read(bytes: &[u8]) -> Result<Loaded, Error> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let mut xml = String::new();
    match zip.by_name("word/document.xml") {
        Ok(mut f) => f.read_to_string(&mut xml)?,
        Err(_) => return Err(Error::NotDocx),
    };
    let (parsed, paras) = parse_document(&xml)?;
    if let Some(ex) = find_extras(&mut zip)
        && let Some(doc) = merge(parsed.clone(), paras.clone(), &ex)
    {
        return Ok(Loaded { doc, seed: Some(ex.seed) });
    }
    let chars = parsed.into_iter().enumerate().map(|(i, (ch, attrs))| Char { ch, id: i as u64, attrs }).collect();
    let doc = Document::from_loaded(chars, paras).ok_or_else(|| Error::Xml("inconsistent paragraphs".into()))?;
    Ok(Loaded { doc, seed: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Command;

    fn sample() -> Document {
        let mut d = Document::from_text("你好，chixie-rs！\n第二段\t制表 & <符号>\n");
        let red = Attrs { colour: Some([200, 0, 0]), size_pt: Some(14.5), font: Some("楷体".into()), v_offset: Some(-0.7), combine_next: false, tuning: None };
        d.apply(Command::SetAttrs { at: 0, attrs: vec![red; 2] });
        // 合并内容 on "ch" (custom part only).
        let joined = Attrs { combine_next: true, ..Attrs::default() };
        d.apply(Command::SetAttrs { at: 3, attrs: vec![joined] });
        d.apply(Command::SetParas { first: 1, paras: vec![Para { align: Align::Center, first_indent: Some(16.25) }] });
        // Ids no longer 0..n.
        let x = d.new_chars("啊", &Attrs::default());
        d.apply(d.splice_cmd(1..1, x));
        d
    }

    #[test]
    fn round_trip_is_exact_and_deterministic() {
        let d = sample();
        let bytes = write(&d, 42).unwrap();
        assert_eq!(bytes, write(&d, 42).unwrap(), "same input, same bytes");
        let l = read(&bytes).unwrap();
        assert!(l.doc.same_content(&d), "{:?}", l.doc.text());
        assert_eq!(l.seed, Some(42));
    }

    #[test]
    fn word_view_has_text_runs_and_paragraphs() {
        let xml = document_xml(&sample());
        assert_eq!(xml.matches("<w:p>").count(), 3);
        assert!(xml.contains(r#"<w:rFonts w:ascii="楷体""#));
        assert!(xml.contains(r#"<w:color w:val="C80000"/>"#));
        assert!(xml.contains(r#"<w:sz w:val="29"/>"#));
        assert!(xml.contains(r#"<w:jc w:val="center"/>"#));
        assert!(xml.contains("<w:tab/>"));
        assert!(xml.contains("&amp; &lt;符号&gt;"));
        // Without the extras, Word's view alone gives the same text and close formatting.
        let (chars, paras) = parse_document(&xml).unwrap();
        assert_eq!(chars.iter().map(|c| c.0).collect::<String>(), sample().text());
        assert_eq!(chars[0].1.colour, Some([200, 0, 0]));
        assert_eq!(chars[0].1.size_pt, Some(14.5));
        assert!((chars[0].1.v_offset.unwrap() + 0.7).abs() < 0.2);
        assert_eq!(paras[1].align, Align::Center);
        assert!((paras[1].first_indent.unwrap() - 16.25).abs() < 0.02);
    }

    #[test]
    fn edits_made_elsewhere_keep_the_untouched_ids() {
        let d = sample();
        let ex = Extras { version: 1, seed: 7, text: d.text(), ids: rle_ids(d.ids()), styles: vec![Attrs::default()], attrs: vec![(0, d.len() as u32)], paras: d.paras().to_vec() };
        // Word user replaced "chixie" with "XY".
        let new_text = d.text().replace("chixie", "XY");
        let parsed: Vec<(char, Attrs)> = new_text.chars().map(|c| (c, Attrs::default())).collect();
        let m = merge(parsed, d.paras().to_vec(), &ex).unwrap();
        assert_eq!(m.text(), new_text);
        let pre = d.text().chars().position(|c| c == 'c').unwrap();
        assert_eq!(&m.ids()[..pre], &d.ids()[..pre]);
        assert_eq!(m.ids()[m.len() - 1], d.ids()[d.len() - 1]);
        assert!(m.ids()[pre] > *d.ids().iter().max().unwrap(), "changed span gets fresh ids");
        assert!(m.is_consistent());
    }

    #[test]
    fn foreign_docx_opens_with_fresh_ids() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("word/document.xml", SimpleFileOptions::default()).unwrap();
        z.write_all(
            br#"<w:document xmlns:w="x"><w:body><w:p><w:pPr><w:jc w:val="right"/><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:t>A&#x4E2D;</w:t><w:br/><w:t>B</w:t></w:r></w:p><w:p/></w:body></w:document>"#,
        )
        .unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let l = read(&bytes).unwrap();
        assert_eq!(l.doc.text(), "A中\nB\n");
        assert_eq!(l.doc.ids(), &[0, 1, 2, 3, 4]);
        assert_eq!(l.seed, None);
        assert_eq!(l.doc.paras().iter().map(|p| p.align).collect::<Vec<_>>(), vec![Align::Right, Align::Right, Align::Left]);
    }

    #[test]
    fn empty_document_round_trips() {
        let d = Document::default();
        let l = read(&write(&d, 1).unwrap()).unwrap();
        assert!(l.doc.same_content(&d));
    }

    #[test]
    fn not_a_docx() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("a.txt", SimpleFileOptions::default()).unwrap();
        let bytes = z.finish().unwrap().into_inner();
        assert!(matches!(read(&bytes), Err(Error::NotDocx)));
    }
}
