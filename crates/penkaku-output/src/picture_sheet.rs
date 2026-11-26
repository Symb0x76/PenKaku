//! 图片文档: the pictures of a report, printed at exactly the size of the spaces the pen leaves for
//! them (`Document::pictures`), packed tightly onto A4 sheets, each with a thin cut frame and its
//! 「图k」 under it, to cut out and glue in.

use std::sync::Arc;

use lopdf::{Dictionary, Object, Stream, dictionary};
use penkaku_core::font::fontset::Glyphs;
use penkaku_core::layout::{self, EM_UNITS, PageParams};

const PT_PER_MM: f32 = 72.0 / 25.4;

/// One picture to print: its number, size (mm) and image bytes (`None` or an unreadable format:
/// an empty framed box, so the numbering still matches).
#[derive(Debug, Clone)]
pub struct SheetItem {
    pub number: u32,
    pub w: f32,
    pub h: f32,
    pub data: Option<Arc<[u8]>>,
}

/// Sheet and spacing, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetOptions {
    pub paper: (f32, f32),
    pub margin: f32,
    /// Between neighbours, room for the scissors.
    pub gap: f32,
    /// Under each picture, for its 「图k」.
    pub label: f32,
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self { paper: (210.0, 297.0), margin: 10.0, gap: 5.0, label: 5.0 }
    }
}

/// Where a picture goes: sheet, top-left (mm from the sheet's top-left) and the item.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub sheet: usize,
    pub x: f32,
    pub y: f32,
    /// Index into the items given to [`pack`].
    pub item: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x - 1e-4 && o.y >= self.y - 1e-4 && o.x + o.w <= self.x + self.w + 1e-4 && o.y + o.h <= self.y + self.h + 1e-4
    }
    fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// MaxRects, best short side fit: place items (w, h) in `order` onto as many `area`-sized bins as
/// needed. Returns (bin, x, y) per item, in item order.
fn max_rects(sizes: &[(f32, f32)], order: &[usize], area: (f32, f32)) -> Vec<(usize, f32, f32)> {
    let mut out = vec![(0, 0.0, 0.0); sizes.len()];
    let mut bins: Vec<Vec<Rect>> = Vec::new();
    for &k in order {
        let (w, h) = (sizes[k].0.min(area.0), sizes[k].1.min(area.1));
        // Best (short leftover, long leftover) over every free rectangle of every open bin.
        let best = bins
            .iter()
            .enumerate()
            .flat_map(|(b, free)| free.iter().filter(|r| r.w >= w - 1e-4 && r.h >= h - 1e-4).map(move |r| (b, *r)))
            .min_by(|(_, a), (_, c)| {
                let key = |r: &Rect| ((r.w - w).min(r.h - h), (r.w - w).max(r.h - h), r.y, r.x);
                key(a).partial_cmp(&key(c)).unwrap_or(std::cmp::Ordering::Equal)
            });
        let (b, at) = match best {
            Some((b, r)) => (b, (r.x, r.y)),
            None => {
                bins.push(vec![Rect { x: 0.0, y: 0.0, w: area.0, h: area.1 }]);
                (bins.len() - 1, (0.0, 0.0))
            }
        };
        let used = Rect { x: at.0, y: at.1, w, h };
        let free = &mut bins[b];
        let mut next = Vec::new();
        for r in free.drain(..) {
            if !r.overlaps(&used) {
                next.push(r);
                continue;
            }
            // The parts of `r` left, right, above and below `used`.
            if used.x > r.x {
                next.push(Rect { w: used.x - r.x, ..r });
            }
            if used.x + used.w < r.x + r.w {
                next.push(Rect { x: used.x + used.w, w: r.x + r.w - used.x - used.w, ..r });
            }
            if used.y > r.y {
                next.push(Rect { h: used.y - r.y, ..r });
            }
            if used.y + used.h < r.y + r.h {
                next.push(Rect { y: used.y + used.h, h: r.y + r.h - used.y - used.h, ..r });
            }
        }
        // Drop free rectangles inside others.
        let keep: Vec<bool> = (0..next.len()).map(|i| !next.iter().enumerate().any(|(j, o)| j != i && o.contains(&next[i]) && (next[i] != *o || j < i))).collect();
        *free = next.into_iter().zip(keep).filter_map(|(r, k)| k.then_some(r)).collect();
        out[k] = (b, at.0, at.1);
    }
    out
}

/// Pack `items` onto sheets: each takes its size plus the label strip and the gap; tries a few
/// orders (tallest, largest, widest first) and keeps the one with the fewest sheets (then the
/// emptiest last sheet). Never rotates, so a cut-out matches its space as it stands. A picture
/// larger than a sheet's free area is placed at the top-left of a sheet of its own.
pub fn pack(items: &[SheetItem], o: &SheetOptions) -> Vec<Placement> {
    let area = (o.paper.0 - 2.0 * o.margin + o.gap, o.paper.1 - 2.0 * o.margin + o.gap);
    let sizes: Vec<(f32, f32)> = items.iter().map(|it| (it.w + o.gap, it.h + o.label + o.gap)).collect();
    let by = |key: &dyn Fn(&(f32, f32)) -> f32| {
        let mut v: Vec<usize> = (0..items.len()).collect();
        v.sort_by(|&a, &b| key(&sizes[b]).partial_cmp(&key(&sizes[a])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        v
    };
    let orders = [by(&|s| s.1), by(&|s| s.0 * s.1), by(&|s| s.0)];
    let score = |p: &[(usize, f32, f32)]| {
        let n = p.iter().map(|q| q.0 + 1).max().unwrap_or(0);
        let last: f32 = p.iter().zip(&sizes).filter(|(q, _)| q.0 + 1 == n).map(|(_, s)| s.0 * s.1).sum();
        (n, last)
    };
    let best = orders
        .iter()
        .map(|ord| max_rects(&sizes, ord, area))
        .min_by(|a, b| {
            let (sa, sb) = (score(a), score(b));
            sa.0.cmp(&sb.0).then(sa.1.partial_cmp(&sb.1).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap_or_default();
    best.into_iter().enumerate().map(|(item, (sheet, x, y))| Placement { sheet, x: o.margin + x, y: o.margin + y, item }).collect()
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// An image as a PDF XObject: JPEG as it is, PNG decoded (RGB, alpha as a soft mask). `None` for
/// what we cannot draw.
fn image_object(out: &mut lopdf::Document, bytes: &[u8]) -> Option<lopdf::ObjectId> {
    match bytes {
        [0xFF, 0xD8, ..] => {
            let (w, h, comps) = jpeg_size(bytes)?;
            let space = match comps {
                1 => "DeviceGray",
                4 => "DeviceCMYK",
                _ => "DeviceRGB",
            };
            let dict = dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => space, "BitsPerComponent" => 8, "Filter" => "DCTDecode" };
            Some(out.add_object(Stream::new(dict, bytes.to_vec())))
        }
        [0x89, b'P', b'N', b'G', ..] => {
            let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.into_rgba8();
            let (w, h) = img.dimensions();
            let (mut rgb, mut alpha) = (Vec::with_capacity((w * h * 3) as usize), Vec::with_capacity((w * h) as usize));
            for p in img.pixels() {
                rgb.extend_from_slice(&p.0[..3]);
                alpha.push(p.0[3]);
            }
            let mut dict = dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 };
            if alpha.iter().any(|&a| a < 255) {
                let mut mask = Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 }, alpha);
                let _ = mask.compress();
                dict.set("SMask", out.add_object(mask));
            }
            let mut s = Stream::new(dict, rgb);
            let _ = s.compress();
            Some(out.add_object(s))
        }
        _ => None,
    }
}

/// Width, height and components from a JPEG's start-of-frame marker.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            let h = u32::from(b[i + 5]) << 8 | u32::from(b[i + 6]);
            let w = u32::from(b[i + 7]) << 8 | u32::from(b[i + 8]);
            return Some((w, h, b[i + 9]));
        }
        let len = (usize::from(b[i + 2]) << 8) | usize::from(b[i + 3]);
        i += 2 + len;
    }
    None
}

/// 「图`number`」 in the handwriting font, `h` mm tall, its top-left at (0, 0); strokes in mm, y
/// down.
fn label_strokes(font: &(impl Glyphs + ?Sized), number: u32, h: f32) -> Vec<Vec<(f32, f32)>> {
    let p = PageParams { mm_per_unit: h / EM_UNITS, origin: (0.0, 0.0), ..Default::default() };
    let doc = layout::layout(&format!("图{number}"), font, &p);
    doc.pages.into_iter().flat_map(|pg| pg.strokes).map(|s| s.into_iter().map(|(x, y)| (x, -y)).collect()).collect()
}

/// The picture sheet as PDF: one page per sheet of [`pack`]; each picture at its size with a thin
/// grey cut frame and 「图k」 (in `font`, the writing font) under it.
pub fn picture_pdf(items: &[SheetItem], font: &(impl Glyphs + ?Sized), o: &SheetOptions) -> Vec<u8> {
    let places = pack(items, o);
    let sheets = places.iter().map(|p| p.sheet + 1).max().unwrap_or(1);
    let (pw, ph) = o.paper;
    let mut out = lopdf::Document::with_version("1.5");
    let pages_id = out.new_object_id();
    let images: Vec<Option<lopdf::ObjectId>> = items.iter().map(|it| it.data.as_deref().and_then(|b| image_object(&mut out, b))).collect();
    let mut kids: Vec<Object> = Vec::new();
    for sheet in 0..sheets {
        let mut ops = String::from("1 J 1 j\n");
        let mut xobjects = Dictionary::new();
        for p in places.iter().filter(|p| p.sheet == sheet) {
            let it = &items[p.item];
            // PDF pt, y up from the sheet's foot.
            let (x, y_low) = (p.x * PT_PER_MM, (ph - p.y - it.h) * PT_PER_MM);
            let (w, h) = (it.w * PT_PER_MM, it.h * PT_PER_MM);
            if let Some(id) = images[p.item] {
                let name = format!("Im{}", p.item);
                ops += &format!("q {} 0 0 {} {} {} cm /{name} Do Q\n", num(w), num(h), num(x), num(y_low));
                xobjects.set(name, id);
            }
            ops += &format!("q 0.6 0.6 0.6 RG {} w [2 2] 0 d {} {} {} {} re S Q\n", num(0.2 * PT_PER_MM), num(x), num(y_low), num(w), num(h));
            let label = label_strokes(font, it.number, (o.label * 0.7).max(1.0));
            if !label.is_empty() {
                ops += &format!("q 0 0 0 RG {} w\n", num(0.15 * PT_PER_MM));
                for s in &label {
                    for (k, &(lx, ly)) in s.iter().enumerate() {
                        let (sx, sy) = ((p.x + lx) * PT_PER_MM, (ph - (p.y + it.h + 0.8 + ly)) * PT_PER_MM);
                        ops += &format!("{} {} {}", num(sx), num(sy), if k == 0 { "m " } else { "l " });
                    }
                    if s.len() == 1 {
                        ops += &format!("{} {} l ", num((p.x + s[0].0 + 0.05) * PT_PER_MM), num((ph - (p.y + it.h + 0.8 + s[0].1)) * PT_PER_MM));
                    }
                    ops += "S\n";
                }
                ops += "Q\n";
            }
        }
        let content = out.add_object(Stream::new(Dictionary::new(), ops.into_bytes()));
        let media: Vec<Object> = vec![0.into(), 0.into(), Object::Real(pw * PT_PER_MM), Object::Real(ph * PT_PER_MM)];
        let resources = dictionary! { "XObject" => xobjects };
        let id = out.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "MediaBox" => media, "Resources" => resources });
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
    use penkaku_core::font::{Font, Glyph};

    fn item(number: u32, w: f32, h: f32) -> SheetItem {
        SheetItem { number, w, h, data: None }
    }

    /// The report's 18 picture spaces (mm).
    fn report() -> Vec<SheetItem> {
        let sizes = [(51.0, 51.0), (41.0, 41.0), (41.0, 41.0), (41.0, 41.0), (70.0, 32.1), (70.0, 32.1), (47.0, 47.0), (47.0, 47.0), (47.0, 47.0), (47.0, 47.0), (45.0, 36.9), (45.0, 36.9), (45.0, 36.9), (94.0, 43.1), (51.0, 47.3), (51.0, 47.3), (51.0, 47.3), (51.0, 47.3)];
        sizes.iter().enumerate().map(|(k, &(w, h))| item(k as u32 + 1, w, h)).collect()
    }

    #[test]
    fn packing_stays_on_the_sheet_never_overlaps_and_keeps_the_gap() {
        let o = SheetOptions::default();
        let items = report();
        let p = pack(&items, &o);
        assert_eq!(p.len(), items.len());
        for (i, a) in p.iter().enumerate() {
            let it = &items[a.item];
            assert!(a.x >= o.margin - 1e-3 && a.y >= o.margin - 1e-3);
            assert!(a.x + it.w <= o.paper.0 - o.margin + 1e-3 && a.y + it.h + o.label <= o.paper.1 - o.margin + 1e-3, "{a:?}");
            for b in &p[i + 1..] {
                if a.sheet != b.sheet {
                    continue;
                }
                let jt = &items[b.item];
                let apart = a.x + it.w + o.gap <= b.x + 1e-3 || b.x + jt.w + o.gap <= a.x + 1e-3 || a.y + it.h + o.label + o.gap <= b.y + 1e-3 || b.y + jt.h + o.label + o.gap <= a.y + 1e-3;
                assert!(apart, "{a:?} {b:?}");
            }
        }
        // 18 pictures, ~3.6 A4 sheets' worth of free area at 100 %: two sheets suffice.
        let sheets = p.iter().map(|q| q.sheet + 1).max().unwrap();
        assert_eq!(sheets, 2);
    }

    #[test]
    fn pdf_has_a4_pages_and_one_image_per_picture() {
        // A 2×1 PNG and a hand-made baseline JPEG header (SOF0, 4×3, 3 components).
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8, 0, 3, 0, 4, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9];
        let items = vec![SheetItem { data: Some(png.into()), ..item(1, 40.0, 20.0) }, SheetItem { data: Some(jpeg.into()), ..item(2, 30.0, 30.0) }, item(3, 20.0, 20.0)];
        let font = Font { glyphs: [('图', Glyph { advance: 7.0, strokes: vec![vec![(0.0, 0.0), (7.0, 7.0)]] })].into_iter().collect() };
        let bytes = picture_pdf(&items, &font, &SheetOptions::default());
        let d = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(d.get_pages().len(), 1);
        let images = d.objects.values().filter(|o| o.as_stream().is_ok_and(|s| s.dict.get(b"Subtype").and_then(|v| v.as_name()).is_ok_and(|n| n == b"Image") && s.dict.get(b"SMask").is_err() && s.dict.get(b"Width").is_ok())).count();
        assert_eq!(images, 2, "PNG and JPEG; picture 3 has no bytes");
        let page = d.get_object(*d.get_pages().values().next().unwrap()).unwrap().as_dict().unwrap();
        let mb: Vec<f32> = page.get(b"MediaBox").unwrap().as_array().unwrap().iter().map(|o| o.as_float().unwrap()).collect();
        assert!((mb[2] - 595.28).abs() < 0.1 && (mb[3] - 841.89).abs() < 0.1, "A4: {mb:?}");
        assert_eq!(jpeg_size(&[0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8, 0, 3, 0, 4, 3]), Some((4, 3, 3)));
    }
}
