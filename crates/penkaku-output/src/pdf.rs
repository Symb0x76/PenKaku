//! 导出 PDF / 打印: the laid-out pages as vector PDF at real size. 打印文字 draws the handwriting
//! strokes; 打印背景图 draws the template's ruling and cell borders, to print your own 作文纸 or
//! forms. Layout mm (origin at the writable top-left, y up) sit inside the page margins.

use penkaku_core::layout::Document;
use lopdf::{Dictionary, Object, Stream, dictionary};

const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Debug, Clone, PartialEq)]
pub struct PdfOptions {
    /// 打印文字.
    pub ink: bool,
    /// 打印背景图.
    pub background: bool,
    /// Sheet (w, h) and margins (left, top, right, bottom), mm.
    pub paper: (f32, f32),
    pub margins: (f32, f32, f32, f32),
    /// Pen width, mm.
    pub pen_mm: f32,
    /// Pen colour per char (stroke colours follow `Page::stroke_chars`); missing = black.
    pub colours: Vec<Option<[u8; 3]>>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self { ink: true, background: true, paper: (210.0, 297.0), margins: (21.0, 20.0, 21.0, 20.0), pen_mm: 0.35, colours: Vec::new() }
    }
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// One PDF page per layout page.
pub fn pdf(doc: &Document, o: &PdfOptions) -> Vec<u8> {
    let mut out = lopdf::Document::with_version("1.5");
    let pages_id = out.new_object_id();
    let (w, h) = o.paper;
    // Layout mm → PDF pt (y up from the sheet's bottom).
    let at = |x: f32, y: f32| (num((o.margins.0 + x) * PT_PER_MM), num((h - (o.margins.1 - y)) * PT_PER_MM));
    let mut kids: Vec<Object> = Vec::new();
    for page in doc.pages.iter() {
        let mut ops = String::from("1 J 1 j\n");
        if o.background && !page.guides.is_empty() {
            ops += &format!("q 0.6 0.6 0.6 RG {} w\n", num(0.2 * PT_PER_MM));
            for dashed in [false, true] {
                let segs: Vec<_> = page.guides.iter().filter(|g| g.dashed == dashed).collect();
                if segs.is_empty() {
                    continue;
                }
                ops += if dashed { "[2 2] 0 d\n" } else { "[] 0 d\n" };
                for g in segs {
                    let ((x0, y0), (x1, y1)) = (at(g.a.0, g.a.1), at(g.b.0, g.b.1));
                    ops += &format!("{x0} {y0} m {x1} {y1} l\n");
                }
                ops += "S\n";
            }
            ops += "Q\n";
        }
        if o.ink && !page.strokes.is_empty() {
            ops += &format!("{} w\n", num(o.pen_mm * PT_PER_MM));
            let colour_of = |i: usize| page.stroke_chars.get(i).and_then(|&c| o.colours.get(c).copied().flatten());
            let mut inks: Vec<Option<[u8; 3]>> = vec![None];
            for i in 0..page.strokes.len() {
                if let Some(c) = colour_of(i).filter(|c| !inks.contains(&Some(*c))) {
                    inks.push(Some(c));
                }
            }
            for ink in inks {
                let [r, g, b] = ink.unwrap_or([0, 0, 0]);
                ops += &format!("{} {} {} RG\n", num(r as f32 / 255.0), num(g as f32 / 255.0), num(b as f32 / 255.0));
                for (_, s) in page.strokes.iter().enumerate().filter(|(i, _)| colour_of(*i) == ink) {
                    let Some(&(x, y)) = s.first() else { continue };
                    let (x0, y0) = at(x, y);
                    ops += &format!("{x0} {y0} m");
                    if s.len() == 1 {
                        // A dot: a tiny segment with round caps.
                        let (x1, y1) = at(x + 0.05, y);
                        ops += &format!(" {x1} {y1} l");
                    }
                    for &(x, y) in &s[1..] {
                        let (x1, y1) = at(x, y);
                        ops += &format!(" {x1} {y1} l");
                    }
                    ops += " S\n";
                }
            }
        }
        let content = out.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
        let media: Vec<Object> = vec![0.into(), 0.into(), Object::Real(w * PT_PER_MM), Object::Real(h * PT_PER_MM)];
        let id = out.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "MediaBox" => media, "Resources" => Dictionary::new() });
        kids.push(id.into());
    }
    let count = kids.len() as i64;
    out.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog = out.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    out.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    out.save_to(&mut bytes).expect("PDF to memory");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use penkaku_core::layout::{Guide, Page};

    fn sample() -> Document {
        let page = |n: usize| Page {
            strokes: (0..n).map(|k| vec![(k as f32, -5.0), (k as f32 + 1.0, -6.0)]).collect(),
            guides: vec![Guide { a: (0.0, 0.0), b: (100.0, 0.0), dashed: false }, Guide { a: (0.0, -10.0), b: (100.0, -10.0), dashed: true }],
            stroke_chars: (0..n).collect(),
        };
        Document { pages: vec![page(3), page(2)], ..Default::default() }
    }

    fn strokes_on(bytes: &[u8]) -> Vec<usize> {
        let d = lopdf::Document::load_mem(bytes).unwrap();
        d.get_pages()
            .values()
            .map(|&p| {
                let c = d.get_page_content(p);
                String::from_utf8_lossy(&c).lines().filter(|l| l.ends_with(" S") && l.contains(" m ")).count()
            })
            .collect()
    }

    #[test]
    fn pages_size_and_stroke_counts() {
        let bytes = pdf(&sample(), &PdfOptions::default());
        let d = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(d.get_pages().len(), 2);
        let first = *d.get_pages().values().next().unwrap();
        let mb = d.get_dictionary(first).unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
        let side = |o: &Object| o.as_float().unwrap();
        assert!((side(&mb[2]) - 595.28).abs() < 0.1 && (side(&mb[3]) - 841.89).abs() < 0.1);
        // One path per stroke, as the layout has them.
        assert_eq!(strokes_on(&bytes), vec![3, 2]);
    }

    #[test]
    fn background_only_has_no_ink() {
        let o = PdfOptions { ink: false, ..Default::default() };
        let bytes = pdf(&sample(), &o);
        assert_eq!(strokes_on(&bytes), vec![0, 0]);
        let d = lopdf::Document::load_mem(&bytes).unwrap();
        let c = d.get_page_content(*d.get_pages().values().next().unwrap());
        let s = String::from_utf8_lossy(&c);
        assert!(s.contains("[2 2] 0 d") && s.contains(" l\n"));
        // Text only: no ruling.
        let bytes = pdf(&sample(), &PdfOptions { background: false, ..Default::default() });
        let d = lopdf::Document::load_mem(&bytes).unwrap();
        let c = d.get_page_content(*d.get_pages().values().next().unwrap());
        assert!(!String::from_utf8_lossy(&c).contains("0.6 0.6 0.6 RG"));
    }

    #[test]
    fn layout_points_land_inside_the_margins() {
        // Writable top-left (0, 0) = (left margin, sheet height − top margin) in pt.
        let doc = Document { pages: vec![Page { strokes: vec![vec![(0.0, 0.0), (10.0, -10.0)]], guides: vec![], stroke_chars: vec![0] }], ..Default::default() };
        let bytes = pdf(&doc, &PdfOptions::default());
        let d = lopdf::Document::load_mem(&bytes).unwrap();
        let c = d.get_page_content(*d.get_pages().values().next().unwrap());
        let s = String::from_utf8_lossy(&c).to_string();
        assert!(s.contains("59.53 785.2 m 87.87 756.85 l S"), "{s}");
    }
}
