//! 书写优化: reshape each pen-down polyline before it becomes G-code. A machine-smoothness choice
//! (fewer, evenly spaced or rounder moves), so it lives in the device profile, not in layout.

use serde::{Deserialize, Serialize};

pub type P = (f32, f32);

/// ChiXie3's 优化规则.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    /// 无优化.
    #[default]
    None,
    /// 简化路径: Douglas-Peucker, `value` = tolerance (mm).
    Simplify,
    /// 优化方案1: Catmull-Rom through the points, `value` = points added per segment (扩张数量).
    CatmullRom,
    /// 优化方案2: points every `value` mm along the stroke.
    Uniform,
}

impl RouteMode {
    pub const ALL: [RouteMode; 4] = [RouteMode::None, RouteMode::Simplify, RouteMode::CatmullRom, RouteMode::Uniform];

    pub fn label(self) -> &'static str {
        match self {
            RouteMode::None => "无优化",
            RouteMode::Simplify => "简化路径",
            RouteMode::CatmullRom => "优化方案1（平滑）",
            RouteMode::Uniform => "优化方案2（均匀）",
        }
    }

    /// A sensible `value` for the mode.
    pub fn default_value(self) -> f32 {
        match self {
            RouteMode::None => 0.0,
            RouteMode::Simplify => 0.02,
            RouteMode::CatmullRom => 4.0,
            RouteMode::Uniform => 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteImprove {
    pub mode: RouteMode,
    pub value: f32,
}

impl RouteImprove {
    /// The stroke as it should be written; `None` when unchanged (borrow the original).
    pub fn apply(&self, s: &[P]) -> Option<Vec<P>> {
        match self.mode {
            RouteMode::None => None,
            RouteMode::Simplify if self.value > 0.0 => Some(simplify_dp(s, self.value)),
            RouteMode::CatmullRom if self.value >= 1.0 => Some(catmull_rom(s, self.value.round() as usize)),
            RouteMode::Uniform if self.value > 0.0 => Some(resample_uniform(s, self.value)),
            _ => None,
        }
    }
}

fn dist(a: P, b: P) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Douglas-Peucker: drop points within `eps` of the line through their neighbours. Endpoints
/// always stay.
pub fn simplify_dp(s: &[P], eps: f32) -> Vec<P> {
    if s.len() < 3 {
        return s.to_vec();
    }
    let mut keep = vec![false; s.len()];
    keep[0] = true;
    keep[s.len() - 1] = true;
    let mut todo = vec![(0, s.len() - 1)];
    while let Some((a, b)) = todo.pop() {
        let (mut far, mut at) = (0.0f32, 0);
        for i in a + 1..b {
            let d = seg_dist(s[i], s[a], s[b]);
            if d > far {
                (far, at) = (d, i);
            }
        }
        if far > eps {
            keep[at] = true;
            todo.push((a, at));
            todo.push((at, b));
        }
    }
    s.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Centripetal-free (uniform) Catmull-Rom through every point, `per_seg` extra points per segment.
pub fn catmull_rom(s: &[P], per_seg: usize) -> Vec<P> {
    if s.len() < 3 || per_seg == 0 {
        return s.to_vec();
    }
    let at = |i: isize| s[i.clamp(0, s.len() as isize - 1) as usize];
    let mut out = Vec::with_capacity(s.len() * (per_seg + 1));
    for i in 0..s.len() - 1 {
        let (p0, p1, p2, p3) = (at(i as isize - 1), s[i], s[i + 1], at(i as isize + 2));
        out.push(p1);
        for k in 1..=per_seg {
            let t = k as f32 / (per_seg + 1) as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3);
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    out.push(s[s.len() - 1]);
    out
}

/// Points every `step` along the polyline (first and last kept).
pub fn resample_uniform(s: &[P], step: f32) -> Vec<P> {
    if s.len() < 2 || step <= 0.0 {
        return s.to_vec();
    }
    let mut out = vec![s[0]];
    let mut need = step;
    for w in s.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = dist(a, b);
        let mut t = 0.0;
        while len - t >= need {
            t += need;
            let f = t / len;
            out.push((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f));
            need = step;
        }
        need -= len - t;
    }
    let last = s[s.len() - 1];
    if out.last().is_none_or(|&p| dist(p, last) > step * 1e-3) {
        out.push(last);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wiggle() -> Vec<P> {
        (0..=40).map(|i| (i as f32 * 0.25, if i % 2 == 0 { 0.0 } else { 0.005 })).collect()
    }

    #[test]
    fn dp_keeps_endpoints_and_drops_noise() {
        let s = wiggle();
        let out = simplify_dp(&s, 0.02);
        assert_eq!(out, vec![s[0], s[40]]);
        let corner = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)];
        assert_eq!(simplify_dp(&corner, 0.02), corner);
    }

    #[test]
    fn uniform_spacing_within_one_percent() {
        let s = vec![(0.0, 0.0), (3.0, 0.0), (3.0, 4.07)];
        let out = resample_uniform(&s, 0.2);
        for w in out.windows(2).take(out.len() - 2) {
            // Spacing along the path; corners cut the chord a little, so compare away from them.
            let d = dist(w[0], w[1]);
            assert!((d - 0.2).abs() < 0.2 * 0.01 || w[1].0 == 3.0, "{d}");
        }
        assert_eq!((out[0], *out.last().unwrap()), ((0.0, 0.0), (3.0, 4.07)));
    }

    #[test]
    fn catmull_rom_passes_through_the_points() {
        let s = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)];
        let out = catmull_rom(&s, 3);
        assert_eq!(out.len(), 3 * 4 + 1);
        for (k, p) in s.iter().enumerate() {
            assert_eq!(out[k * 4], *p);
        }
    }

    #[test]
    fn none_changes_nothing() {
        assert_eq!(RouteImprove::default().apply(&wiggle()), None);
    }
}
