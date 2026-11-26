//! Flowcharts drawn by the pen: a Mermaid subset ([`mermaid`]) placed in ranks ([`graph`]) and
//! turned into clean polylines plus label positions ([`draw`]). Layout adds the handwriting
//! waver and writes the labels with the document's font.

pub mod graph;
pub mod mermaid;

use graph::{Placed, Pt, Spacing};
use mermaid::{Graph, Shape};

/// One label to write: its text and the centre of its box (mm, y down).
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub center: Pt,
}

/// A flowchart ready to write: outline and edge strokes, labels, bounding size (mm, y down,
/// everything inside `0..size`).
#[derive(Debug, Clone, Default)]
pub struct Drawing {
    pub strokes: Vec<Vec<Pt>>,
    /// Which strokes are straight-edged outlines and links (`true`) vs arrowheads (`false`);
    /// only the former waver.
    pub waver: Vec<bool>,
    pub labels: Vec<Label>,
    pub size: (f32, f32),
}

/// Draw `g` with text `em` mm tall; `measure(text)` is the (w, h) of a label at that size.
pub fn draw(g: &Graph, em: f32, measure: &dyn Fn(&str) -> (f32, f32)) -> Drawing {
    let (px, py) = (em * 0.8, em * 0.5);
    let sizes: Vec<(f32, f32)> = g
        .nodes
        .iter()
        .map(|n| {
            let (tw, th) = measure(&n.label);
            let (w, h) = ((tw + 2.0 * px).max(em * 3.0), (th + 2.0 * py).max(em * 1.6));
            match n.shape {
                Shape::Diamond => (w * 1.5, h * 1.5),
                Shape::Circle => {
                    let d = (tw.hypot(th) + 2.0 * py).max(em * 2.0);
                    (d, d)
                }
                Shape::Stadium => (w + h * 0.5, h),
                _ => (w, h),
            }
        })
        .collect();
    let edge_labels: Vec<Option<(f32, f32)>> = g.edges.iter().map(|e| e.label.as_deref().map(measure)).collect();
    let tallest = edge_labels.iter().flatten().map(|s| s.1).fold(0.0, f32::max);
    let widest = edge_labels.iter().flatten().map(|s| s.0).fold(0.0, f32::max);
    let horiz = matches!(g.dir, mermaid::Dir::Right | mermaid::Dir::Left);
    let room = if horiz { widest } else { tallest };
    let sp = Spacing { node_gap: em * 1.5 + if horiz { tallest } else { 0.0 }, rank_gap: em * 2.0 + room + if room > 0.0 { em * 0.6 } else { 0.0 } };
    let Placed { centers, edges, size } = graph::place(g, &sizes, sp);

    let mut d = Drawing { size, ..Drawing::default() };
    for ((n, &c), &s) in g.nodes.iter().zip(&centers).zip(&sizes) {
        d.strokes.push(outline(n.shape, c, s));
        d.waver.push(true);
        if !n.label.is_empty() {
            d.labels.push(Label { text: n.label.clone(), center: c });
        }
    }
    let (dash, gap) = (em * 0.35, em * 0.25);
    let mut top = 0.0f32;
    for ((e, pts), lab) in g.edges.iter().zip(&edges).zip(&edge_labels) {
        if pts.len() < 2 {
            continue;
        }
        if e.dashed {
            for p in dashes(pts, dash, gap) {
                d.strokes.push(p);
                d.waver.push(false);
            }
        } else {
            d.strokes.push(pts.clone());
            d.waver.push(true);
        }
        if e.arrow {
            d.strokes.push(arrowhead(pts, em * 0.5));
            d.waver.push(false);
        }
        if let (Some(text), Some((lw, lh))) = (&e.label, lab) {
            // Beside the link's midpoint, so the pen never writes over the line.
            let (m, dir) = midpoint(pts);
            let center = if dir.0.abs() > dir.1.abs() { (m.0, m.1 - lh / 2.0 - em * 0.3) } else { (m.0 + lw / 2.0 + em * 0.4, m.1) };
            d.size.0 = d.size.0.max(center.0 + lw / 2.0);
            d.size.1 = d.size.1.max(center.1 + lh / 2.0);
            top = top.min(center.1 - lh / 2.0);
            d.labels.push(Label { text: text.clone(), center });
        }
    }
    // Labels above a horizontal link may poke out of the top.
    if top < 0.0 {
        let lift = -top;
        for p in d.strokes.iter_mut().flatten() {
            p.1 += lift;
        }
        for l in &mut d.labels {
            l.center.1 += lift;
        }
        d.size.1 += lift;
    }
    d
}

/// Closed outline of a node centred at `c` with size `s`.
pub fn outline(shape: Shape, (cx, cy): Pt, (w, h): (f32, f32)) -> Vec<Pt> {
    let (x0, y0, x1, y1) = (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
    let rounded = |r: f32| {
        let r = r.min(w / 2.0).min(h / 2.0);
        // Corners clockwise from top-right; each arc a quarter turn in 4 steps.
        let corners = [((x1 - r, y0 + r), -90.0f32), ((x1 - r, y1 - r), 0.0), ((x0 + r, y1 - r), 90.0), ((x0 + r, y0 + r), 180.0)];
        let mut v: Vec<Pt> = vec![(x0 + r, y0)];
        for ((ox, oy), a0) in corners {
            for k in 0..=4 {
                let a = (a0 + 22.5 * k as f32).to_radians();
                v.push((ox + r * a.cos(), oy + r * a.sin()));
            }
        }
        v.push((x0 + r, y0));
        v
    };
    match shape {
        Shape::Rect => vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)],
        Shape::Round => rounded(h * 0.25),
        Shape::Stadium => rounded(h / 2.0),
        Shape::Diamond => vec![(cx, y0), (x1, cy), (cx, y1), (x0, cy), (cx, y0)],
        Shape::Circle => (0..=32).map(|k| {
            let a = (k as f32 / 32.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            (cx + w / 2.0 * a.cos(), cy + h / 2.0 * a.sin())
        }).collect(),
    }
}

/// One stroke `left → tip → right` at the polyline's end.
fn arrowhead(pts: &[Pt], len: f32) -> Vec<Pt> {
    let tip = pts[pts.len() - 1];
    let prev = pts[pts.len() - 2];
    let (dx, dy) = (tip.0 - prev.0, tip.1 - prev.1);
    let l = dx.hypot(dy).max(1e-6);
    let (ux, uy) = (dx / l, dy / l);
    let (c, s) = (25f32.to_radians().cos(), 25f32.to_radians().sin());
    let back = |sg: f32| (tip.0 - len * (ux * c - sg * uy * s), tip.1 - len * (uy * c + sg * ux * s));
    vec![back(1.0), tip, back(-1.0)]
}

/// The point halfway along the polyline and the direction of the segment it lies on.
fn midpoint(pts: &[Pt]) -> (Pt, Pt) {
    let seg = |w: &[Pt]| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
    let total: f32 = pts.windows(2).map(seg).sum();
    let mut left = total / 2.0;
    for w in pts.windows(2) {
        let l = seg(w);
        if left <= l && l > 0.0 {
            let t = left / l;
            return ((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t), (w[1].0 - w[0].0, w[1].1 - w[0].1));
        }
        left -= l;
    }
    (pts[0], (0.0, 1.0))
}

/// The polyline cut into `dash`-long pieces `gap` apart.
fn dashes(pts: &[Pt], dash: f32, gap: f32) -> Vec<Vec<Pt>> {
    let mut out = Vec::new();
    let mut cur: Vec<Pt> = vec![pts[0]];
    let (mut on, mut left) = (true, dash);
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        loop {
            let l = (b.0 - a.0).hypot(b.1 - a.1);
            if l <= left {
                left -= l;
                if on {
                    cur.push(b);
                }
                break;
            }
            let t = left / l;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if on {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            } else {
                cur = vec![p];
            }
            on = !on;
            left = if on { dash } else { gap };
            a = p;
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(t: &str) -> (f32, f32) {
        let lines = t.lines().count().max(1) as f32;
        (t.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 5.0, lines * 6.0)
    }

    #[test]
    fn draws_outlines_links_arrows_and_labels_inside_the_box() {
        let g = mermaid::parse("flowchart TD\nA[开始] --> B{是否}\nB -->|是| C\nB -.->|否| A").unwrap();
        let d = draw(&g, 5.0, &measure);
        assert_eq!(d.strokes.len(), d.waver.len());
        // 3 outlines + 2 solid links + dashes + 3 arrowheads.
        assert!(d.strokes.len() > 3 + 2 + 3);
        assert_eq!(d.labels.len(), 3 + 2);
        for &(x, y) in d.strokes.iter().flatten() {
            assert!(x >= -1e-3 && y >= -1e-3 && x <= d.size.0 + 1e-3 && y <= d.size.1 + 1e-3, "({x}, {y}) outside {:?}", d.size);
        }
    }

    #[test]
    fn dashes_cover_the_line_with_gaps() {
        let d = dashes(&[(0.0, 0.0), (10.0, 0.0)], 2.0, 1.0);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0], vec![(0.0, 0.0), (2.0, 0.0)]);
        assert_eq!(d[1][0], (3.0, 0.0));
    }

    #[test]
    fn outlines_close() {
        for s in [Shape::Rect, Shape::Round, Shape::Stadium, Shape::Diamond, Shape::Circle] {
            let o = outline(s, (10.0, 10.0), (12.0, 6.0));
            let (a, b) = (o[0], o[o.len() - 1]);
            assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3, "{s:?} closes");
        }
    }
}
