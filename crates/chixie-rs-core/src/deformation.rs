//! Character deformation (ChiXie3 设置 → 字符变形), modelled from captures (docs/protocol.md):
//!
//! - **Base** (基础变形): per-glyph random width/height scale (percent) and rotation (degrees)
//!   about the glyph centre, plus a small per-stroke shift (笔画变形幅度). Captures: scales near
//!   96–104%, rotation within ±2°, 0.02–0.06 mm stroke-level residual at amplitude 1.
//! - **Shape** (字形变形): CJK glyphs get a consistent slant (shear ≈ −0.12…−0.13 at amplitude 7)
//!   plus smooth non-affine noise (0.035–0.106 mm residual at 7). Latin was barely affected.
//!
//! The stroke-shift and shape-noise *forms* are best guesses that match the measured magnitudes;
//! their exact formulas are VERIFY. "Remove small tails" is not modelled (no effect seen).

use serde::{Deserialize, Serialize};

use crate::handwriting::{Rng, Span};
use crate::layout::Stroke;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Deformation {
    pub enabled: bool,
    pub base: bool,
    pub shape: bool,
    /// Width scale, percent (ChiXie3 字符随机胖瘦).
    pub h_scale_pct: Span,
    /// Height scale, percent (ChiXie3 字符随机高矮).
    pub v_scale_pct: Span,
    /// Rotation, degrees; positive = counter-clockwise on paper.
    pub rotation_deg: Span,
    /// Stroke deformation amplitude (笔画变形幅度), slider 0–2; 0 disables the stroke shift.
    pub stroke_degree: f32,
    /// Shape deformation amplitude (字形变形幅度), slider 0–10; 0 disables shape deformation.
    pub shape_degree: f32,
    /// 随机删除小尾巴: with base deformation, sometimes drop a stroke's short end hook.
    pub remove_tails: bool,
}

impl Default for Deformation {
    /// ChiXie3's shipped defaults, disabled.
    fn default() -> Self {
        Self {
            enabled: false,
            base: false,
            shape: false,
            h_scale_pct: Span::new(96.0, 104.0),
            v_scale_pct: Span::new(96.0, 104.0),
            rotation_deg: Span::new(-2.0, 2.0),
            stroke_degree: 1.0,
            shape_degree: 7.0,
            remove_tails: true,
        }
    }
}

/// Max per-stroke shift in mm per unit of `stroke_degree` (captures: ~0.02–0.06 mm RMS at 1).
const STROKE_SHIFT_MM: f32 = 0.06;
/// Shear per unit of `shape_degree` (captures: −0.118…−0.134 at 7).
const SHAPE_SHEAR: Span = Span::new(0.016, 0.0195);
/// Smooth-noise amplitude in mm per unit of `shape_degree` (captures: up to ~0.1 mm at 7).
const SHAPE_NOISE_MM: f32 = 0.012;

impl Deformation {
    /// ChiXie3's 推荐参数 (its shipped config): 胖瘦 96–104 %, 高矮 98–102 %, rotation ±2°,
    /// 笔画变形幅度 1, 字形变形幅度 7, 随机删除小尾巴 on. Switches off, like a fresh install.
    pub fn recommended() -> Self {
        Self { v_scale_pct: Span::new(98.0, 102.0), ..Self::default() }
    }

    pub fn is_active(&self) -> bool {
        self.enabled && (self.base || (self.shape && self.shape_degree > 0.0))
    }

    /// Deform one glyph's strokes (glyph units, y down). `mm_per_unit` converts the mm-valued
    /// stroke shift and noise into glyph units. Draws from `rng` in a fixed order.
    pub fn apply(&self, strokes: &[Stroke], cjk: bool, mm_per_unit: f32, rng: &mut Rng) -> Vec<Stroke> {
        let mut out: Vec<Stroke> = strokes.to_vec();
        if !self.is_active() || out.iter().all(|s| s.is_empty()) {
            return out;
        }
        let (cx, cy, w, h) = bbox(&out);
        let unit = |mm: f32| if mm_per_unit > 0.0 { mm / mm_per_unit } else { 0.0 };

        if self.base {
            let sx = self.h_scale_pct.sample(rng) / 100.0;
            let sy = self.v_scale_pct.sample(rng) / 100.0;
            // Glyph space is y-down, so a counter-clockwise turn on paper is clockwise here.
            let th = -self.rotation_deg.sample(rng).to_radians();
            let (sin, cos) = th.sin_cos();
            for p in out.iter_mut().flatten() {
                let (dx, dy) = ((p.0 - cx) * sx, (p.1 - cy) * sy);
                *p = (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
            }
            if self.stroke_degree > 0.0 {
                for s in &mut out {
                    let r = unit(STROKE_SHIFT_MM * self.stroke_degree) * rng.unit();
                    let a = rng.unit() * std::f32::consts::TAU;
                    let (ox, oy) = (r * a.cos(), r * a.sin());
                    for p in s.iter_mut() {
                        *p = (p.0 + ox, p.1 + oy);
                    }
                }
            }
        }

        if self.shape && self.shape_degree > 0.0 && cjk {
            // Slant: on paper x shifts right as y goes up; glyph y is down, so use (cy - y).
            let k = SHAPE_SHEAR.sample(rng) * self.shape_degree;
            let amp = unit(SHAPE_NOISE_MM * self.shape_degree);
            let warp = Tps::random(amp, rng);
            let (w, h) = (w.max(1e-3), h.max(1e-3));
            for p in out.iter_mut().flatten() {
                let (x, y) = *p;
                let (nx, ny) = warp.at((x - cx) / w, (y - cy) / h);
                *p = (x + k * (cy - y) + nx, y + ny);
            }
        }

        if self.base && self.remove_tails {
            // Its own stream, drawn after everything above, so the other effects are unchanged.
            let mut trng = Rng::new(rng.next_u64());
            let max_len = unit(TAIL_MAX_MM);
            for s in &mut out {
                if trng.unit() < TAIL_P {
                    drop_tail(s, max_len);
                }
            }
        }
        out
    }
}

/// Control points per side of the thin-plate-spline grid.
const TPS_N: usize = 4;

/// The non-affine part of a thin-plate spline through random displacements of a 4×4 grid over
/// the glyph box (coordinates normalised to −½…½): a smooth warp, like a hand that bends a
/// character slightly differently each time. Its affine part is dropped, because slant and
/// scale are modelled separately.
struct Tps {
    ctrl: [(f64, f64); TPS_N * TPS_N],
    wx: [f64; TPS_N * TPS_N],
    wy: [f64; TPS_N * TPS_N],
}

fn tps_u(r2: f64) -> f64 {
    if r2 <= 1e-12 { 0.0 } else { 0.5 * r2 * r2.ln() }
}

impl Tps {
    /// Displacements uniform in ±`amp` (glyph units) at every control point.
    fn random(amp: f32, rng: &mut Rng) -> Self {
        const M: usize = TPS_N * TPS_N;
        let mut ctrl = [(0.0, 0.0); M];
        for (i, c) in ctrl.iter_mut().enumerate() {
            let (col, row) = (i % TPS_N, i / TPS_N);
            *c = (col as f64 / (TPS_N - 1) as f64 - 0.5, row as f64 / (TPS_N - 1) as f64 - 0.5);
        }
        let d: Vec<(f64, f64)> = (0..M).map(|_| (f64::from((rng.unit() * 2.0 - 1.0) * amp), f64::from((rng.unit() * 2.0 - 1.0) * amp))).collect();
        // [K P; Pᵀ 0] [w; a] = [d; 0], solved for x and y together.
        const S: usize = M + 3;
        let mut a = [[0.0f64; S + 2]; S];
        for i in 0..M {
            for j in 0..M {
                let (dx, dy) = (ctrl[i].0 - ctrl[j].0, ctrl[i].1 - ctrl[j].1);
                a[i][j] = tps_u(dx * dx + dy * dy);
            }
            let p = [1.0, ctrl[i].0, ctrl[i].1];
            for k in 0..3 {
                a[i][M + k] = p[k];
                a[M + k][i] = p[k];
            }
            a[i][S] = d[i].0;
            a[i][S + 1] = d[i].1;
        }
        solve(&mut a);
        let (mut wx, mut wy) = ([0.0; M], [0.0; M]);
        for i in 0..M {
            (wx[i], wy[i]) = (a[i][S], a[i][S + 1]);
        }
        Self { ctrl, wx, wy }
    }

    /// The warp's displacement at normalised (x, y).
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = (f64::from(x), f64::from(y));
        let (mut dx, mut dy) = (0.0, 0.0);
        for (i, c) in self.ctrl.iter().enumerate() {
            let u = tps_u((x - c.0).powi(2) + (y - c.1).powi(2));
            dx += self.wx[i] * u;
            dy += self.wy[i] * u;
        }
        (dx as f32, dy as f32)
    }
}

/// Gauss-Jordan elimination with partial pivoting on an augmented matrix `[A | B]` (B is the
/// last two columns); leaves the solution in those columns.
fn solve<const S: usize, const W: usize>(a: &mut [[f64; W]; S]) {
    for col in 0..S {
        let piv = (col..S).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs())).unwrap_or(col);
        a.swap(col, piv);
        let p = a[col][col];
        if p.abs() < 1e-12 {
            continue;
        }
        for v in a[col].iter_mut() {
            *v /= p;
        }
        let pivot_row = a[col];
        for (r, row) in a.iter_mut().enumerate() {
            if r != col && row[col] != 0.0 {
                let f = row[col];
                for (v, pv) in row.iter_mut().zip(pivot_row.iter()) {
                    *v -= f * pv;
                }
            }
        }
    }
}

/// Chance per stroke that its end hook is dropped (随机删除小尾巴). Best guess, VERIFY.
const TAIL_P: f32 = 0.3;
/// A hook is at most this long (mm).
const TAIL_MAX_MM: f32 = 0.3;

/// Drop a short end hook: the trailing points within `max_len` of the stroke's end, when they
/// turn back more than 90° from the direction the stroke was going. At least three points stay.
fn drop_tail(s: &mut Stroke, max_len: f32) {
    let n = s.len();
    if n < 4 || max_len <= 0.0 {
        return;
    }
    let dist = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
    // `j`: first point of the tail; the tail is s[j..], its length < max_len.
    let mut j = n - 1;
    let mut len = 0.0;
    while j > 2 {
        let d = dist(s[j - 1], s[j]);
        if len + d > max_len {
            break;
        }
        len += d;
        j -= 1;
    }
    if j == n - 1 || len <= 0.0 {
        return;
    }
    let (a, b, end) = (s[j - 1], s[j], s[n - 1]);
    let main = (b.0 - a.0, b.1 - a.1);
    let tail = (end.0 - b.0, end.1 - b.1);
    if main.0 * tail.0 + main.1 * tail.1 < 0.0 {
        s.truncate(j + 1);
    }
}

/// (centre x, centre y, width, height) of all points.
fn bbox(strokes: &[Stroke]) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in strokes.iter().flatten() {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Stroke> {
        vec![vec![(0.0, 0.0), (4.0, 0.0), (4.0, 6.0), (0.0, 6.0), (0.0, 0.0)]]
    }

    #[test]
    fn disabled_is_identity() {
        let d = Deformation { base: true, ..Default::default() }; // enabled = false
        assert_eq!(d.apply(&square(), true, 1.0, &mut Rng::new(1)), square());
    }

    #[test]
    fn base_scale_stays_in_range_without_rotation_or_shift() {
        let d = Deformation {
            enabled: true,
            base: true,
            rotation_deg: Span::new(0.0, 0.0),
            stroke_degree: 0.0,
            ..Default::default()
        };
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let out = d.apply(&square(), true, 1.0, &mut rng);
            let (_, _, w, h) = bbox(&out);
            assert!((4.0 * 0.96 - 1e-4..=4.0 * 1.04 + 1e-4).contains(&w), "w {w}");
            assert!((6.0 * 0.96 - 1e-4..=6.0 * 1.04 + 1e-4).contains(&h), "h {h}");
        }
    }

    #[test]
    fn rotation_is_bounded() {
        let d = Deformation {
            enabled: true,
            base: true,
            h_scale_pct: Span::new(100.0, 100.0),
            v_scale_pct: Span::new(100.0, 100.0),
            stroke_degree: 0.0,
            ..Default::default()
        };
        let mut rng = Rng::new(5);
        for _ in 0..200 {
            let out = d.apply(&[vec![(0.0, 0.0), (10.0, 0.0)]], false, 1.0, &mut rng);
            let (a, b) = (out[0][0], out[0][1]);
            let deg = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
            assert!(deg.abs() <= 2.0 + 1e-3, "deg {deg}");
        }
    }

    #[test]
    fn shape_slants_cjk_only() {
        let d = Deformation { enabled: true, shape: true, ..Default::default() };
        let vertical = vec![vec![(0.0, 0.0), (0.0, 10.0)]]; // top then bottom (y down)
        let cjk = d.apply(&vertical, true, 1.0, &mut Rng::new(9));
        // Top point moves right relative to the bottom point: shear ~0.11–0.14 * 10 units.
        let lean = (cjk[0][0].0 - cjk[0][1].0) / 10.0;
        assert!((0.10..=0.15).contains(&lean), "lean {lean}");
        assert_eq!(d.apply(&vertical, false, 1.0, &mut Rng::new(9)), vertical);
    }

    #[test]
    fn hooks_are_dropped_straight_strokes_kept() {
        // A long stroke right, then a 0.2-unit hook back left-down (turn > 90°).
        let mut hooked = vec![(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (3.9, 0.15)];
        drop_tail(&mut hooked, 0.3);
        assert_eq!(hooked, vec![(0.0, 0.0), (2.0, 0.0), (4.0, 0.0)]);
        let straight = vec![(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (4.2, 0.0)];
        let mut s = straight.clone();
        drop_tail(&mut s, 0.3);
        assert_eq!(s, straight, "no turn, no tail");
        // Through `apply`: some hooks go, stable per seed, none when switched off.
        let d = Deformation {
            enabled: true,
            base: true,
            h_scale_pct: Span::new(100.0, 100.0),
            v_scale_pct: Span::new(100.0, 100.0),
            rotation_deg: Span::new(0.0, 0.0),
            stroke_degree: 0.0,
            ..Default::default()
        };
        let glyph = vec![vec![(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (3.9, 0.15)]];
        let cut = (0..200).filter(|&k| d.apply(&glyph, true, 1.0, &mut Rng::new(k))[0].len() == 3).count();
        assert!((30..=90).contains(&cut), "{cut} of 200 hooks dropped");
        assert_eq!(d.apply(&glyph, true, 1.0, &mut Rng::new(5)), d.apply(&glyph, true, 1.0, &mut Rng::new(5)));
        let off = Deformation { remove_tails: false, ..d };
        assert!((0..50).all(|k| off.apply(&glyph, true, 1.0, &mut Rng::new(k))[0].len() == 4));
    }

    /// A dense CJK-sized glyph (6×6 mm at 1 unit = 1 mm) as 7 strokes of 13 points.
    fn dense() -> Vec<Stroke> {
        (0..7).map(|r| (0..13).map(|c| (c as f32 * 0.5, r as f32 * 1.0)).collect()).collect()
    }

    /// RMS distance left after the best affine fit of `out` to `inp` (the non-affine warp).
    fn affine_residual(inp: &[Stroke], out: &[Stroke]) -> f64 {
        let ps: Vec<(f64, f64)> = inp.iter().flatten().map(|p| (f64::from(p.0), f64::from(p.1))).collect();
        let qs: Vec<(f64, f64)> = out.iter().flatten().map(|p| (f64::from(p.0), f64::from(p.1))).collect();
        let n = ps.len() as f64;
        let (mx, my) = (ps.iter().map(|p| p.0).sum::<f64>() / n, ps.iter().map(|p| p.1).sum::<f64>() / n);
        let (sxx, syy, sxy) = ps.iter().fold((0.0, 0.0, 0.0), |(a, b, c), p| (a + (p.0 - mx).powi(2), b + (p.1 - my).powi(2), c + (p.0 - mx) * (p.1 - my)));
        let det = sxx * syy - sxy * sxy;
        let mut err = 0.0;
        for k in 0..2 {
            let q = |i: usize| if k == 0 { qs[i].0 } else { qs[i].1 };
            let nq = (0..qs.len()).map(q).sum::<f64>() / n;
            let bx: f64 = ps.iter().enumerate().map(|(i, p)| (p.0 - mx) * (q(i) - nq)).sum();
            let by: f64 = ps.iter().enumerate().map(|(i, p)| (p.1 - my) * (q(i) - nq)).sum();
            let (a, b) = ((bx * syy - by * sxy) / det, (by * sxx - bx * sxy) / det);
            err += ps.iter().enumerate().map(|(i, p)| (a * (p.0 - mx) + b * (p.1 - my) + nq - q(i)).powi(2)).sum::<f64>();
        }
        (err / n).sqrt()
    }

    #[test]
    fn shape_warp_magnitude_follows_the_level() {
        // Captured at level 7 (2026-11-06): 0.035–0.106 mm non-affine residual on CJK.
        let res = |level: f32| {
            let d = Deformation { enabled: true, shape: true, shape_degree: level, ..Default::default() };
            let v: Vec<f64> = (0..40).map(|k| affine_residual(&dense(), &d.apply(&dense(), true, 1.0, &mut Rng::new(k)))).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        let (r3, r7, r10) = (res(3.0), res(7.0), res(10.0));
        assert!((0.02..=0.106).contains(&r7), "level 7 residual {r7}");
        assert!(r3 < r7 && r7 < r10, "{r3} {r7} {r10}");
        let off = Deformation { enabled: true, shape: true, shape_degree: 0.0, ..Default::default() };
        assert_eq!(off.apply(&dense(), true, 1.0, &mut Rng::new(1)), dense());
    }

    /// Effects must stay cheap: < 50 µs per glyph (release builds; debug is far slower).
    #[cfg(not(debug_assertions))]
    #[test]
    fn deforming_a_glyph_takes_under_50_us() {
        let d = Deformation { enabled: true, base: true, shape: true, ..Default::default() };
        let glyph = dense();
        let t0 = std::time::Instant::now();
        let mut n = 0;
        for k in 0..10_000u64 {
            n += d.apply(&glyph, true, 0.37, &mut Rng::new(k)).len();
        }
        let per = t0.elapsed() / 10_000;
        assert!(n > 0 && per < std::time::Duration::from_micros(50), "{per:?} per glyph");
    }

    #[test]
    fn same_seed_same_result() {
        let d = Deformation { enabled: true, base: true, shape: true, ..Default::default() };
        let a = d.apply(&square(), true, 1.0, &mut Rng::new(11));
        let b = d.apply(&square(), true, 1.0, &mut Rng::new(11));
        assert_eq!(a, b);
        assert_ne!(a, d.apply(&square(), true, 1.0, &mut Rng::new(12)));
    }
}
