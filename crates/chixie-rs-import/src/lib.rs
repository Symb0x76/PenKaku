//! Table frames from PDFs, scans and photos, detected locally (ChiXie3 sends them to a cloud
//! service instead). Two paths meet in [`grid`]:
//! - vector PDFs: the page's line and rectangle drawing operators (`pdf`);
//! - images and scanned PDFs: threshold, perspective fix, line masks (`raster`).
//!
//! The result is plain geometry; the GUI turns it into a table template.

pub mod grid;
pub mod pdf;
pub mod raster;
#[doc(hidden)]
pub mod synth;

pub use grid::{DetectedTable, HSeg, VSeg};
use image::GrayImage;

/// One page of an imported file.
#[derive(Debug, Clone)]
pub struct DetectedPage {
    /// The page size when the file says it (PDF).
    pub size_mm: Option<(f32, f32)>,
    /// Millimetres per table unit: 1 for vector PDFs, the page scale for scanned PDFs, `None` for
    /// images and unwarped photos (the import dialog asks for the table width).
    pub mm_per_unit: Option<f32>,
    /// The source page is wider than tall.
    pub landscape: bool,
    pub tables: Vec<DetectedTable>,
    /// The (unwarped) page image, for the template background and 内边距识别.
    pub background: Option<GrayImage>,
}

impl DetectedPage {
    fn from_image(img: &GrayImage, mm_per_px: Option<f32>, size_mm: Option<(f32, f32)>) -> Self {
        let r = raster::detect(img);
        // An unwarped page has lost the page's own scale.
        let mm = if r.warped { None } else { mm_per_px };
        let (w, h) = r.image.dimensions();
        Self { size_mm, mm_per_unit: mm, landscape: w > h, tables: r.tables, background: Some(r.image) }
    }
}

/// Detect the tables of a PDF (by its `%PDF` header) or an image (PNG / JPEG).
pub fn detect(bytes: &[u8]) -> Result<Vec<DetectedPage>, String> {
    if bytes.starts_with(b"%PDF") {
        return Ok(pdf::read(bytes)?
            .into_iter()
            .map(|p| match &p.scan {
                Some(img) if p.tables.is_empty() => DetectedPage::from_image(img, Some(p.size_mm.0 / img.width() as f32), Some(p.size_mm)),
                _ => DetectedPage { size_mm: Some(p.size_mm), mm_per_unit: Some(1.0), landscape: p.size_mm.0 > p.size_mm.1, tables: p.tables, background: None },
            })
            .collect());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_luma8();
    Ok(vec![DetectedPage::from_image(&img, None, None)])
}

/// 内边距识别: how far (px) the first ink lies from the left edge of a cell's inside
/// `(x0, y0, x1, y1)` in `img`. The cell's own border lines (within `border` px of the edges) and
/// isolated specks are ignored; `None` when the cell is empty.
pub fn left_padding(img: &GrayImage, rect: (u32, u32, u32, u32), border: u32) -> Option<u32> {
    let (x0, y0, x1, y1) = rect;
    let (x1, y1) = (x1.min(img.width()), y1.min(img.height()));
    let (ya, yb) = (y0 + border, y1.saturating_sub(border));
    if x0 + border >= x1 || ya >= yb {
        return None;
    }
    // Paper level: the cell's brightest tenth.
    let mut px: Vec<u8> = (ya..yb).flat_map(|y| (x0 + border..x1.saturating_sub(border)).map(move |x| (x, y))).map(|(x, y)| img.get_pixel(x, y).0[0]).collect();
    px.sort_unstable();
    let paper = px[px.len() * 9 / 10] as i32;
    let dark = |x: u32, y: u32| (img.get_pixel(x, y).0[0] as i32) < paper - 70;
    (x0 + border..x1.saturating_sub(border)).find(|&x| (ya..yb).filter(|&y| dark(x, y)).count() >= 2).map(|x| x - x0)
}

/// ChiXie3's message for pages where no table was found (1-based page numbers).
pub fn failed_pages_message(pages: &[DetectedPage]) -> Option<String> {
    let bad: Vec<String> = pages.iter().enumerate().filter(|(_, p)| p.tables.is_empty()).map(|(k, _)| (k + 1).to_string()).collect();
    (!bad.is_empty()).then(|| format!("{} 页表格解析异常，请手动创建该页面表格", bad.join("、")))
}
