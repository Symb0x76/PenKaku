//! Images (screenshots, scans, phone photos): adaptive threshold → find the table frame → unwarp
//! its perspective → long horizontal / vertical ink runs → [`grid`]. Units are pixels of the
//! unwarped image, which is returned too (the page background).

use image::{GrayImage, Luma};

use crate::grid::{self, DetectedTable, HSeg, VSeg};

/// Tables found on an image, in pixels of `image` (the unwarped page).
#[derive(Debug, Clone)]
pub struct RasterResult {
    pub tables: Vec<DetectedTable>,
    pub image: GrayImage,
    /// The perspective was corrected.
    pub warped: bool,
}

/// Sauvola threshold (integral images): `true` = ink.
pub fn binarize(img: &GrayImage) -> Vec<bool> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut sum = vec![0f64; (w + 1) * (h + 1)];
    let mut sq = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let (mut rs, mut rq) = (0f64, 0f64);
        for x in 0..w {
            let p = img.get_pixel(x as u32, y as u32).0[0] as f64;
            rs += p;
            rq += p * p;
            sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + rs;
            sq[(y + 1) * (w + 1) + x + 1] = sq[y * (w + 1) + x + 1] + rq;
        }
    }
    let r = (w.min(h) / 40).clamp(7, 40);
    let mut out = vec![false; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            let at = |t: &[f64], xx: usize, yy: usize| t[yy * (w + 1) + xx];
            let s = at(&sum, x1, y1) - at(&sum, x0, y1) - at(&sum, x1, y0) + at(&sum, x0, y0);
            let q = at(&sq, x1, y1) - at(&sq, x0, y1) - at(&sq, x1, y0) + at(&sq, x0, y0);
            let mean = s / n;
            let sd = (q / n - mean * mean).max(0.0).sqrt();
            let t = mean * (1.0 + 0.25 * (sd / 128.0 - 1.0));
            out[y * w + x] = (img.get_pixel(x as u32, y as u32).0[0] as f64) < t.min(200.0);
        }
    }
    out
}

/// Connected ink components (8-neighbour, after a 1 px dilation), largest first: pixel lists.
fn components(ink: &[bool], w: usize, h: usize) -> Vec<Vec<(u32, u32)>> {
    let mut grown = ink.to_vec();
    for y in 0..h {
        for x in 0..w {
            if ink[y * w + x] {
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        grown[ny as usize * w + nx as usize] = true;
                    }
                }
            }
        }
    }
    let mut seen = vec![false; w * h];
    let mut out: Vec<Vec<(u32, u32)>> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !grown[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut comp = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            if ink[i] {
                comp.push((x as u32, y as u32));
            }
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let j = ny as usize * w + nx as usize;
                        if grown[j] && !seen[j] {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        out.push(comp);
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.len()));
    out
}

/// Corners of a component: top-left, top-right, bottom-right, bottom-left (extremes of x ± y).
fn corners(comp: &[(u32, u32)]) -> [(f64, f64); 4] {
    let key = |f: &dyn Fn(f64, f64) -> f64| {
        let p = comp.iter().max_by(|a, b| f(a.0 as f64, a.1 as f64).total_cmp(&f(b.0 as f64, b.1 as f64))).copied().unwrap_or((0, 0));
        (p.0 as f64, p.1 as f64)
    };
    [key(&|x, y| -(x + y)), key(&|x, y| x - y), key(&|x, y| x + y), key(&|x, y| y - x)]
}

/// Sharper corners: fit a line to the component's pixels along each side of the rough quad
/// (total least squares), then intersect neighbouring sides.
fn refine(comp: &[(u32, u32)], rough: [(f64, f64); 4]) -> [(f64, f64); 4] {
    let span = ((rough[2].0 - rough[0].0).abs() + (rough[2].1 - rough[0].1).abs()).max(1.0);
    let band = (span * 0.012).max(3.0);
    // Side k from corner k to k+1: (point on line, unit direction).
    let mut lines: Vec<((f64, f64), (f64, f64))> = Vec::new();
    for k in 0..4 {
        let (a, b) = (rough[k], rough[(k + 1) % 4]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (ux, uy) = (dx / len, dy / len);
        // Points near the side, away from its ends (corner blobs).
        let pts: Vec<(f64, f64)> = comp
            .iter()
            .map(|&(x, y)| (x as f64, y as f64))
            .filter(|&(x, y)| {
                let (rx, ry) = (x - a.0, y - a.1);
                let along = rx * ux + ry * uy;
                (rx * uy - ry * ux).abs() <= band && along > 0.1 * len && along < 0.9 * len
            })
            .collect();
        if pts.len() < 10 {
            return rough;
        }
        let n = pts.len() as f64;
        let (mx, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for &(x, y) in &pts {
            sxx += (x - mx) * (x - mx);
            syy += (y - my) * (y - my);
            sxy += (x - mx) * (y - my);
        }
        let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
        lines.push(((mx, my), (theta.cos(), theta.sin())));
    }
    let meet = |l1: ((f64, f64), (f64, f64)), l2: ((f64, f64), (f64, f64))| -> Option<(f64, f64)> {
        let ((p, d), (q, e)) = (l1, l2);
        let den = d.0 * e.1 - d.1 * e.0;
        if den.abs() < 1e-9 {
            return None;
        }
        let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / den;
        Some((p.0 + t * d.0, p.1 + t * d.1))
    };
    // Corner k sits between side k−1 and side k.
    let mut out = rough;
    for k in 0..4 {
        match meet(lines[(k + 3) % 4], lines[k]) {
            Some(c) => out[k] = c,
            None => return rough,
        }
    }
    out
}

/// Homography mapping `from[k]` to `to[k]` (4 points): 3×3, row-major, h33 = 1.
pub fn homography(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> Option<[f64; 9]> {
    let mut a = [[0f64; 9]; 8];
    for k in 0..4 {
        let ((x, y), (u, v)) = (from[k], to[k]);
        a[2 * k] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        a[2 * k + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    // Gaussian elimination with partial pivoting on the 8×8 system.
    for col in 0..8 {
        let piv = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        for row in 0..8 {
            if row != col {
                let f = a[row][col] / a[col][col];
                for k in col..9 {
                    a[row][k] -= f * a[col][k];
                }
            }
        }
    }
    let s: Vec<f64> = (0..8).map(|i| a[i][8] / a[i][i]).collect();
    Some([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], 1.0])
}

pub fn project(m: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let d = m[6] * x + m[7] * y + m[8];
    ((m[0] * x + m[1] * y + m[2]) / d, (m[3] * x + m[4] * y + m[5]) / d)
}

fn sample(img: &GrayImage, x: f64, y: f64) -> u8 {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if x < 0.0 || y < 0.0 || x > w - 1.0 || y > h - 1.0 {
        return 255;
    }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let p = |xx: f64, yy: f64| img.get_pixel(xx.min(w - 1.0) as u32, yy.min(h - 1.0) as u32).0[0] as f64;
    let v = p(x0, y0) * (1.0 - fx) * (1.0 - fy) + p(x0 + 1.0, y0) * fx * (1.0 - fy) + p(x0, y0 + 1.0) * (1.0 - fx) * fy + p(x0 + 1.0, y0 + 1.0) * fx * fy;
    v.round().clamp(0.0, 255.0) as u8
}

/// The quad `c` (TL, TR, BR, BL) straightened into a rectangle with a `margin` px border.
fn unwarp(img: &GrayImage, c: [(f64, f64); 4], margin: f64) -> Option<GrayImage> {
    let d = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let w = (d(c[0], c[1]) + d(c[3], c[2])) / 2.0;
    let h = (d(c[0], c[3]) + d(c[1], c[2])) / 2.0;
    if w < 20.0 || h < 20.0 {
        return None;
    }
    let to = [(margin, margin), (margin + w, margin), (margin + w, margin + h), (margin, margin + h)];
    // Map output pixels back into the photo.
    let back = homography(to, c)?;
    let (ow, oh) = ((w + 2.0 * margin).round() as u32, (h + 2.0 * margin).round() as u32);
    Some(GrayImage::from_fn(ow, oh, |x, y| {
        let (sx, sy) = project(&back, x as f64, y as f64);
        Luma([sample(img, sx, sy)])
    }))
}

/// Ink runs of at least `min` px along rows (`horizontal`) or columns; gaps ≤ 2 px are bridged.
fn runs(ink: &[bool], w: usize, h: usize, horizontal: bool, min: usize) -> Vec<(usize, usize, usize)> {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let at = |o: usize, i: usize| if horizontal { ink[o * w + i] } else { ink[i * w + o] };
    let mut out = Vec::new();
    for o in 0..outer {
        let mut i = 0;
        while i < inner {
            if !at(o, i) {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = i;
            let mut gap = 0;
            while i < inner {
                if at(o, i) {
                    end = i;
                    gap = 0;
                } else {
                    gap += 1;
                    if gap > 2 {
                        break;
                    }
                }
                i += 1;
            }
            if end + 1 - start >= min {
                out.push((o, start, end + 1));
            }
        }
    }
    out
}

/// Tables on an already straight image (pixels).
fn tables_on(img: &GrayImage) -> Vec<DetectedTable> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let ink = binarize(img);
    let min = (w.min(h) / 25).max(12);
    let hs: Vec<HSeg> = runs(&ink, w, h, true, min).into_iter().map(|(y, a, b)| HSeg { x0: a as f32, x1: b as f32, y: y as f32 }).collect();
    let vs: Vec<VSeg> = runs(&ink, w, h, false, min).into_iter().map(|(x, a, b)| VSeg { y0: a as f32, y1: b as f32, x: x as f32 }).collect();
    let tol = (w.max(h) as f32 * 0.008).max(3.0);
    grid::build(&hs, &vs, tol)
}

fn cell_count(t: &[DetectedTable]) -> usize {
    t.iter().map(|t| t.cells.len()).sum()
}

/// Mean grey just outside minus just inside the quad's corners: strongly negative for the sheet's
/// own edge against a darker desk.
fn edge_contrast(img: &GrayImage, c: &[(f64, f64); 4]) -> f64 {
    let g = (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0);
    let at = |p: (f64, f64), k: f64| sample(img, p.0 + (p.0 - g.0) * k, p.1 + (p.1 - g.1) * k) as f64;
    c.iter().map(|&p| at(p, 0.03) - at(p, -0.03)).sum::<f64>() / 4.0
}

/// Detect tables on an image. The perspective is corrected from the largest ink frames (the two
/// biggest components are tried): a table's own frame, or the sheet's edge when the sheet lies
/// on a darker desk (then the image is cropped just inside the sheet so its edge drops out).
/// Priority: a table frame (its largest table fills the straightened frame; the smallest such
/// frame wins, as a table lies inside the sheet), then the sheet edge, then the image as it is
/// (a tilted image read straight breaks into bogus cells, so the cell count alone can't decide).
pub fn detect(img: &GrayImage) -> RasterResult {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut frame: Option<(f64, RasterResult)> = None;
    let mut sheet_result: Option<RasterResult> = None;
    let ink = binarize(img);
    for comp in components(&ink, w, h).into_iter().take(2) {
        if comp.len() < 50 {
            break;
        }
        let c = refine(&comp, corners(&comp));
        let sheet = edge_contrast(img, &c) < -12.0;
        // Already axis-aligned and not a sheet edge: nothing to unwarp.
        let skew = (c[0].1 - c[1].1).abs().max((c[3].1 - c[2].1).abs()).max((c[0].0 - c[3].0).abs()).max((c[1].0 - c[2].0).abs());
        if skew < 2.0 && !sheet {
            continue;
        }
        let size = w.min(h) as f64;
        let margin = if sheet { -0.015 * size } else { (0.03 * size).max(10.0) };
        let Some(flat) = unwarp(img, c, margin) else { continue };
        let tables = tables_on(&flat);
        if tables.is_empty() {
            continue;
        }
        if sheet {
            if sheet_result.as_ref().is_none_or(|r| cell_count(&tables) > cell_count(&r.tables)) {
                sheet_result = Some(RasterResult { tables, image: flat, warped: true });
            }
            continue;
        }
        // The frame is the table: its largest table must fill it.
        let (fw, fh) = (flat.width() as f32 - 2.0 * margin as f32, flat.height() as f32 - 2.0 * margin as f32);
        let fills = tables.iter().any(|t| t.width() >= 0.85 * fw && t.height() >= 0.85 * fh);
        let area = ((c[1].0 - c[0].0) * (c[3].1 - c[0].1)).abs();
        if fills && frame.as_ref().is_none_or(|(a, _)| area < *a) {
            frame = Some((area, RasterResult { tables, image: flat, warped: true }));
        }
    }
    frame.map(|f| f.1).or(sheet_result).unwrap_or_else(|| RasterResult { tables: tables_on(img), image: img.clone(), warped: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{self, Form, Shot};

    /// Cells must match; column / row proportions within 1.5 % of the table's size.
    fn matches(found: &DetectedTable, f: &Form) {
        assert_eq!(found.cells, f.cells, "cells");
        let (fw, fh) = (f.xs[f.xs.len() - 1] - f.xs[0], f.ys[f.ys.len() - 1] - f.ys[0]);
        for (k, c) in found.cols.iter().enumerate() {
            let want = (f.xs[k + 1] - f.xs[k]) / fw;
            assert!((c / found.width() - want).abs() < 0.015, "col {k}: {} vs {want}", c / found.width());
        }
        for (k, r) in found.rows.iter().enumerate() {
            let want = (f.ys[k + 1] - f.ys[k]) / fh;
            assert!((r / found.height() - want).abs() < 0.015, "row {k}: {} vs {want}", r / found.height());
        }
    }

    #[test]
    fn homography_maps_the_corners() {
        let from = [(10.0, 20.0), (200.0, 5.0), (230.0, 300.0), (0.0, 280.0)];
        let to = [(0.0, 0.0), (100.0, 0.0), (100.0, 150.0), (0.0, 150.0)];
        let m = homography(from, to).unwrap();
        for k in 0..4 {
            let p = project(&m, from[k].0, from[k].1);
            assert!((p.0 - to[k].0).abs() < 1e-6 && (p.1 - to[k].1).abs() < 1e-6);
        }
    }

    #[test]
    fn clean_screenshot() {
        let f = Form::merged_header();
        let img = synth::render(&f, &Shot::screenshot());
        let r = detect(&img);
        assert_eq!(r.tables.len(), 1);
        matches(&r.tables[0], &f);
    }

    #[test]
    fn tilted_photo_with_perspective() {
        let f = Form::mixed();
        let r = detect(&synth::render(&f, &Shot::photo(4.0, 0.06, 1)));
        assert!(r.warped);
        matches(&r.tables[0], &f);
    }
}
