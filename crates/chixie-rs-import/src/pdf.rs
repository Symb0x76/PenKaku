//! Vector PDFs: the page's drawing operators give the table lines exactly. Stroked `m`/`l`/`re`
//! paths and thin filled rectangles (many PDFs draw borders as filled bars) count when they are
//! axis-parallel; the transformation matrix (`q`/`Q`/`cm`) and form XObjects are followed. A page
//! with no lines but a page-sized image is a scan: its image goes to the raster path.

use image::GrayImage;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::grid::{self, DetectedTable, HSeg, VSeg};

const MM_PER_PT: f32 = 25.4 / 72.0;
/// Filled rectangles thinner than this (pt) are lines.
const BAR_PT: f32 = 2.5;
/// Pieces shorter than this (pt) are ignored (text decorations, dots).
const MIN_PT: f32 = 4.0;
/// Grid tolerance (mm): about a thick line.
pub const TOL_MM: f32 = 0.8;

/// One PDF page: its size, the tables drawn on it (mm, from the top-left), or the scanned image
/// when it has no lines.
#[derive(Debug, Clone)]
pub struct PdfPage {
    pub size_mm: (f32, f32),
    pub tables: Vec<DetectedTable>,
    pub scan: Option<GrayImage>,
}

type Mat = [f32; 6];

fn mul(m: &Mat, n: &Mat) -> Mat {
    // Row vector convention: p' = p × m, then × n.
    [m[0] * n[0] + m[1] * n[2], m[0] * n[1] + m[1] * n[3], m[2] * n[0] + m[3] * n[2], m[2] * n[1] + m[3] * n[3], m[4] * n[0] + m[5] * n[2] + n[4], m[4] * n[1] + m[5] * n[3] + n[5]]
}

fn apply(m: &Mat, x: f32, y: f32) -> (f32, f32) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn nums<const N: usize>(ops: &[Object]) -> Option<[f32; N]> {
    let v: Vec<f32> = ops.iter().filter_map(num).collect();
    v.try_into().ok()
}

/// Lines (device pt, y up) and drawn images found while walking content.
#[derive(Default)]
struct Found {
    h: Vec<(f32, f32, f32)>,
    v: Vec<(f32, f32, f32)>,
    /// (image object, device bbox x0, y0, x1, y1)
    images: Vec<(ObjectId, [f32; 4])>,
}

impl Found {
    fn line(&mut self, a: (f32, f32), b: (f32, f32)) {
        let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
        if dy <= 0.5 && dx >= MIN_PT {
            self.h.push((a.0.min(b.0), a.0.max(b.0), (a.1 + b.1) / 2.0));
        } else if dx <= 0.5 && dy >= MIN_PT {
            self.v.push((a.1.min(b.1), a.1.max(b.1), (a.0 + b.0) / 2.0));
        }
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

fn xobject<'a>(doc: &'a Document, res: &[&'a Dictionary], name: &[u8]) -> Option<(ObjectId, &'a lopdf::Stream)> {
    for r in res {
        let Ok(xo) = r.get(b"XObject") else { continue };
        let Object::Dictionary(xo) = deref(doc, xo) else { continue };
        if let Ok(Object::Reference(id)) = xo.get(name)
            && let Ok(Object::Stream(s)) = doc.get_object(*id)
        {
            return Some((*id, s));
        }
    }
    None
}

fn walk(doc: &Document, content: &[u8], res: &[&Dictionary], ctm0: Mat, found: &mut Found, depth: u32) {
    let Ok(content) = Content::decode(content) else { return };
    let mut ctm = ctm0;
    let mut stack: Vec<Mat> = Vec::new();
    // Current path: segments (device space) and thin-rect candidates.
    let mut segs: Vec<((f32, f32), (f32, f32))> = Vec::new();
    let mut rects: Vec<[(f32, f32); 4]> = Vec::new();
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    for op in &content.operations {
        let o = &op.operands;
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(ctm0),
            "cm" => {
                if let Some(m) = nums::<6>(o) {
                    ctm = mul(&m, &ctm);
                }
            }
            "m" => {
                if let Some([x, y]) = nums::<2>(o) {
                    cur = apply(&ctm, x, y);
                    start = cur;
                }
            }
            "l" => {
                if let Some([x, y]) = nums::<2>(o) {
                    let p = apply(&ctm, x, y);
                    segs.push((cur, p));
                    cur = p;
                }
            }
            "h" => {
                segs.push((cur, start));
                cur = start;
            }
            "re" => {
                if let Some([x, y, w, hh]) = nums::<4>(o) {
                    let c = [apply(&ctm, x, y), apply(&ctm, x + w, y), apply(&ctm, x + w, y + hh), apply(&ctm, x, y + hh)];
                    for k in 0..4 {
                        segs.push((c[k], c[(k + 1) % 4]));
                    }
                    rects.push(c);
                    cur = c[0];
                    start = c[0];
                }
            }
            "S" | "s" | "B" | "B*" | "b" | "b*" => {
                for (a, b) in segs.drain(..) {
                    found.line(a, b);
                }
                rects.clear();
            }
            "f" | "F" | "f*" => {
                // A thin filled rectangle is a line along its long side.
                for c in rects.drain(..) {
                    let (xs, ys) = (c.map(|p| p.0), c.map(|p| p.1));
                    let (x0, x1) = (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max));
                    let (y0, y1) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
                    if y1 - y0 <= BAR_PT && x1 - x0 >= MIN_PT {
                        found.line((x0, (y0 + y1) / 2.0), (x1, (y0 + y1) / 2.0));
                    } else if x1 - x0 <= BAR_PT && y1 - y0 >= MIN_PT {
                        found.line(((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1));
                    }
                }
                segs.clear();
            }
            "n" => {
                segs.clear();
                rects.clear();
            }
            "Do" if depth < 8 => {
                let Some(Object::Name(name)) = o.first() else { continue };
                let Some((id, s)) = xobject(doc, res, name) else { continue };
                match s.dict.get(b"Subtype") {
                    Ok(Object::Name(t)) if t == b"Form" => {
                        let m = s.dict.get(b"Matrix").ok().and_then(|m| match deref(doc, m) {
                            Object::Array(a) => nums::<6>(a),
                            _ => None,
                        });
                        let inner = mul(&m.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]), &ctm);
                        let own = s.dict.get(b"Resources").ok().and_then(|r| match deref(doc, r) {
                            Object::Dictionary(d) => Some(d),
                            _ => None,
                        });
                        let mut res2: Vec<&Dictionary> = own.into_iter().collect();
                        res2.extend_from_slice(res);
                        if let Ok(data) = s.decompressed_content().or_else(|_| Ok::<_, lopdf::Error>(s.content.clone())) {
                            walk(doc, &data, &res2, inner, found, depth + 1);
                        }
                    }
                    Ok(Object::Name(t)) if t == b"Image" => {
                        let c = [apply(&ctm, 0.0, 0.0), apply(&ctm, 1.0, 1.0)];
                        found.images.push((id, [c[0].0.min(c[1].0), c[0].1.min(c[1].1), c[0].0.max(c[1].0), c[0].1.max(c[1].1)]));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn media_box(doc: &Document, page: ObjectId) -> [f32; 4] {
    let mut id = Some(page);
    for _ in 0..16 {
        let Some(d) = id.and_then(|i| doc.get_dictionary(i).ok()) else { break };
        if let Ok(b) = d.get(b"MediaBox")
            && let Object::Array(a) = deref(doc, b)
            && let Some(m) = nums::<4>(a)
        {
            return m;
        }
        id = match d.get(b"Parent") {
            Ok(Object::Reference(p)) => Some(*p),
            _ => None,
        };
    }
    [0.0, 0.0, 595.0, 842.0]
}

/// Decode an image XObject to grey.
pub(crate) fn image_of(s: &lopdf::Stream) -> Option<GrayImage> {
    let filters: Vec<Vec<u8>> = match s.dict.get(b"Filter") {
        Ok(Object::Name(n)) => vec![n.clone()],
        Ok(Object::Array(a)) => a.iter().filter_map(|o| if let Object::Name(n) = o { Some(n.clone()) } else { None }).collect(),
        _ => Vec::new(),
    };
    if filters.iter().any(|f| f == b"DCTDecode") {
        return image::load_from_memory(&s.content).ok().map(|i| i.to_luma8());
    }
    let w = s.dict.get(b"Width").ok().and_then(num)? as u32;
    let h = s.dict.get(b"Height").ok().and_then(num)? as u32;
    let bpc = s.dict.get(b"BitsPerComponent").ok().and_then(num).unwrap_or(8.0) as u32;
    let data = if filters.is_empty() { s.content.clone() } else { s.decompressed_content().ok()? };
    let comps = match s.dict.get(b"ColorSpace") {
        Ok(Object::Name(n)) if n == b"DeviceRGB" => 3,
        Ok(Object::Name(n)) if n == b"DeviceCMYK" => 4,
        _ => 1,
    };
    if bpc != 8 || data.len() < (w * h * comps) as usize {
        return None;
    }
    let grey: Vec<u8> = data
        .chunks(comps as usize)
        .take((w * h) as usize)
        .map(|p| match comps {
            3 => ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8,
            4 => 255 - ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 + p[3] as u32).min(255) as u8,
            _ => p[0],
        })
        .collect();
    GrayImage::from_raw(w, h, grey)
}

/// Every page of a PDF.
pub fn read(bytes: &[u8]) -> Result<Vec<PdfPage>, String> {
    let doc = Document::load_mem(bytes).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (_, page) in doc.get_pages() {
        let mb = media_box(&doc, page);
        let (w_pt, h_pt) = (mb[2] - mb[0], mb[3] - mb[1]);
        let (res, ids) = doc.get_page_resources(page).map_err(|e| e.to_string())?;
        let mut all: Vec<&Dictionary> = res.into_iter().collect();
        all.extend(ids.iter().filter_map(|i| doc.get_dictionary(*i).ok()));
        let mut found = Found::default();
        walk(&doc, &doc.get_page_content(page), &all, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut found, 0);
        // Device pt (y up) → mm from the page's top-left.
        let mx = |x: f32| (x - mb[0]) * MM_PER_PT;
        let my = |y: f32| (mb[3] - y) * MM_PER_PT;
        let h: Vec<HSeg> = found.h.iter().map(|&(x0, x1, y)| HSeg { x0: mx(x0), x1: mx(x1), y: my(y) }).collect();
        let v: Vec<VSeg> = found.v.iter().map(|&(y0, y1, x)| VSeg { y0: my(y1), y1: my(y0), x: mx(x) }).collect();
        let tables = grid::build(&h, &v, TOL_MM);
        // A page-sized image and no table: a scan.
        let scan = if tables.is_empty() {
            found
                .images
                .iter()
                .filter(|(_, b)| (b[2] - b[0]) * (b[3] - b[1]) >= 0.5 * w_pt * h_pt)
                .find_map(|(id, _)| match doc.get_object(*id) {
                    Ok(Object::Stream(s)) => image_of(s),
                    _ => None,
                })
        } else {
            None
        };
        out.push(PdfPage { size_mm: (w_pt * MM_PER_PT, h_pt * MM_PER_PT), tables, scan });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn grid_cells(nr: u32, nc: u32) -> Vec<(u32, u32, u32, u32)> {
        (0..nr).flat_map(|r| (0..nc).map(move |c| (r, c, r + 1, c + 1))).collect()
    }

    fn close(a: &DetectedTable, f: &synth::Form) {
        assert_eq!(a.cells, f.cells, "cells");
        assert!((a.x - f.xs[0]).abs() < 0.5 && (a.y - f.ys[0]).abs() < 0.5, "corner {} {}", a.x, a.y);
        for (k, w) in a.cols.iter().enumerate() {
            assert!((w - (f.xs[k + 1] - f.xs[k])).abs() < 0.5, "col {k}: {w}");
        }
    }

    #[test]
    fn stroked_lines_rects_and_bars() {
        for style in [synth::Style::Lines, synth::Style::Rects, synth::Style::Bars] {
            let f = synth::Form::merged_header();
            let pdf = synth::pdf(&[(f.clone(), style, false)]);
            let pages = read(&pdf).unwrap();
            assert_eq!(pages.len(), 1);
            assert!((pages[0].size_mm.0 - 210.0).abs() < 0.5);
            assert_eq!(pages[0].tables.len(), 1, "{style:?}");
            close(&pages[0].tables[0], &f);
        }
    }

    #[test]
    fn transformed_content_and_two_pages() {
        let a = synth::Form::grid(20.0, 30.0, &[40.0, 40.0], &[12.0; 3]);
        let b = synth::Form::grid(15.0, 50.0, &[30.0; 4], &[10.0; 2]);
        // Page 1 drawn through a scaled + translated `cm` (and a form XObject), page 2 plain.
        let pdf = synth::pdf(&[(a.clone(), synth::Style::Lines, true), (b.clone(), synth::Style::Lines, false)]);
        let pages = read(&pdf).unwrap();
        assert_eq!(pages.len(), 2);
        close(&pages[0].tables[0], &a);
        close(&pages[1].tables[0], &b);
        assert_eq!(pages[1].tables[0].cells, grid_cells(2, 4));
    }

    #[test]
    fn an_image_only_page_is_a_scan() {
        let img = GrayImage::from_fn(300, 400, |x, y| image::Luma([if x == 50 || y == 60 { 0 } else { 255 }]));
        let pdf = synth::pdf_with_image(&img);
        let pages = read(&pdf).unwrap();
        assert!(pages[0].tables.is_empty());
        let scan = pages[0].scan.as_ref().expect("scan image");
        assert_eq!(scan.dimensions(), (300, 400));
        assert_eq!(scan.get_pixel(50, 10).0[0], 0);
    }
}
