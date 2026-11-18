//! Synthetic forms for tests and the GUI autotest: a grid with merged cells, written as a vector
//! PDF in several drawing styles, or rendered as an image and then rotated, warped, blurred and
//! noised like a photo or a scan.

use image::GrayImage;
use lopdf::{Document, Object, Stream, dictionary};

use crate::grid::{self, HSeg, VSeg};

const MM_PER_PT: f32 = 25.4 / 72.0;
const A4_PT: (f32, f32) = (595.276, 841.89);

/// A form: grid lines (mm from the page's top-left) and its cells (spans, end exclusive).
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub cells: Vec<(u32, u32, u32, u32)>,
}

impl Form {
    /// A plain grid at (x, y) mm.
    pub fn grid(x: f32, y: f32, cols: &[f32], rows: &[f32]) -> Self {
        let xs = std::iter::once(x).chain(cols.iter().scan(x, |a, w| { *a += w; Some(*a) })).collect();
        let ys = std::iter::once(y).chain(rows.iter().scan(y, |a, h| { *a += h; Some(*a) })).collect();
        let cells = (0..rows.len() as u32).flat_map(|r| (0..cols.len() as u32).map(move |c| (r, c, r + 1, c + 1))).collect();
        Self { xs, ys, cells }
    }

    /// Replace the cells inside `span` by one merged cell.
    pub fn merge(mut self, span: (u32, u32, u32, u32)) -> Self {
        let (r0, c0, r1, c1) = span;
        self.cells.retain(|&(a, b, _, _)| !(a >= r0 && a < r1 && b >= c0 && b < c1));
        self.cells.push(span);
        self.cells.sort_unstable();
        self
    }

    /// 4 × 3 form with a header across all columns.
    pub fn merged_header() -> Self {
        Form::grid(25.0, 40.0, &[50.0, 60.0, 50.0], &[14.0, 10.0, 10.0, 10.0]).merge((0, 0, 1, 3))
    }

    /// 5 × 4 form: header, a name column merged over rows 1–2, a wide remarks cell at the bottom.
    pub fn mixed() -> Self {
        Form::grid(20.0, 35.0, &[30.0, 45.0, 45.0, 50.0], &[12.0, 10.0, 10.0, 10.0, 18.0]).merge((0, 0, 1, 4)).merge((1, 0, 3, 1)).merge((4, 1, 5, 4))
    }

    /// A dense 6 × 8 grid.
    pub fn dense() -> Self {
        Form::grid(15.0, 30.0, &[22.0; 8], &[9.0; 6])
    }

    /// Ruling segments in mm.
    pub fn segments(&self) -> (Vec<HSeg>, Vec<VSeg>) {
        grid::segments_of(&self.xs, &self.ys, &self.cells)
    }
}

/// How a PDF draws its table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Style {
    /// `m` / `l` / `S` per line.
    Lines,
    /// `re` / `S` per cell.
    Rects,
    /// Thin filled bars (`re` / `f`).
    Bars,
}

fn pt(x_mm: f32, y_mm: f32) -> (f32, f32) {
    (x_mm / MM_PER_PT, A4_PT.1 - y_mm / MM_PER_PT)
}

/// Content drawing `forms`, with `map` turning page pt into the content's own coordinates.
fn content(forms: &[Form], style: Style, map: &dyn Fn((f32, f32)) -> (f32, f32)) -> String {
    let mut s = String::from("0 G 0.8 w\n");
    for f in forms {
        match style {
            Style::Lines | Style::Bars => {
                let (h, v) = f.segments();
                let mut lines: Vec<((f32, f32), (f32, f32))> = h.iter().map(|g| (pt(g.x0, g.y), pt(g.x1, g.y))).collect();
                lines.extend(v.iter().map(|g| (pt(g.x, g.y0), pt(g.x, g.y1))));
                for (a, b) in lines {
                    let (a, b) = (map(a), map(b));
                    if style == Style::Lines {
                        s += &format!("{:.3} {:.3} m {:.3} {:.3} l S\n", a.0, a.1, b.0, b.1);
                    } else if (a.1 - b.1).abs() < 0.01 {
                        s += &format!("{:.3} {:.3} {:.3} 0.8 re f\n", a.0.min(b.0), a.1 - 0.4, (b.0 - a.0).abs());
                    } else {
                        s += &format!("{:.3} {:.3} 0.8 {:.3} re f\n", a.0 - 0.4, a.1.min(b.1), (b.1 - a.1).abs());
                    }
                }
            }
            Style::Rects => {
                for &(r0, c0, r1, c1) in &f.cells {
                    let a = map(pt(f.xs[c0 as usize], f.ys[r1 as usize]));
                    let b = map(pt(f.xs[c1 as usize], f.ys[r0 as usize]));
                    s += &format!("{:.3} {:.3} {:.3} {:.3} re S\n", a.0, a.1, b.0 - a.0, b.1 - a.1);
                }
            }
        }
    }
    s
}

fn finish(mut doc: Document, pages_id: lopdf::ObjectId, kids: Vec<Object>) -> Vec<u8> {
    let count = kids.len() as i64;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("pdf");
    buf
}

fn media() -> Object {
    Object::Array(vec![0.into(), 0.into(), Object::Real(A4_PT.0), Object::Real(A4_PT.1)])
}

/// A4 PDF, one page per entry: the form, how it is drawn, and whether it goes through a scaling
/// `cm` inside a form XObject.
pub fn pdf(pages: &[(Form, Style, bool)]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for (f, style, transformed) in pages {
        let page_dict = if *transformed {
            // Page: q 0.5 0 0 0.5 10 20 cm /Fm0 Do Q; the form draws at (p − (10, 20)) / 0.5.
            let inner = content(std::slice::from_ref(f), *style, &|(x, y)| ((x - 10.0) / 0.5, (y - 20.0) / 0.5));
            let fm = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![Object::Real(-2000.0), Object::Real(-2000.0), Object::Real(4000.0), Object::Real(4000.0)] }, inner.into_bytes()));
            let c = doc.add_object(Stream::new(dictionary! {}, b"q 0.5 0 0 0.5 10 20 cm /Fm0 Do Q".to_vec()));
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => c, "MediaBox" => media(), "Resources" => dictionary! { "XObject" => dictionary! { "Fm0" => fm } } }
        } else {
            let c = doc.add_object(Stream::new(dictionary! {}, content(std::slice::from_ref(f), *style, &|p| p).into_bytes()));
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => c, "MediaBox" => media() }
        };
        kids.push(doc.add_object(page_dict).into());
    }
    finish(doc, pages_id, kids)
}

/// An A4 PDF holding only `img`, stretched over the page (a scanned form).
pub fn pdf_with_image(img: &GrayImage) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let (w, h) = img.dimensions();
    let im = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
        img.as_raw().clone(),
    ));
    let c = doc.add_object(Stream::new(dictionary! {}, format!("q {} 0 0 {} 0 0 cm /Im0 Do Q", A4_PT.0, A4_PT.1).into_bytes()));
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => c, "MediaBox" => media(), "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im } } });
    finish(doc, pages_id, vec![page.into()])
}
