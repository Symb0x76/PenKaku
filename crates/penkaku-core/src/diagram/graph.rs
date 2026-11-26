//! Layered (Sugiyama-style) placement of a flowchart: cycles broken by reversing DFS back edges,
//! longest-path ranks, dummy nodes on long edges, barycenter sweeps against crossings, then
//! x positions pulled toward neighbours under a minimum separation. Coordinates are mm, y down.

use super::mermaid::{Dir, Graph, Shape};

pub type Pt = (f32, f32);

#[derive(Debug, Clone, Copy)]
pub struct Spacing {
    /// Between neighbours in one rank.
    pub node_gap: f32,
    /// Between ranks.
    pub rank_gap: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Placed {
    /// Centre of each node.
    pub centers: Vec<Pt>,
    /// One polyline per edge (graph order), `from` → `to`, ending on the node outlines.
    pub edges: Vec<Vec<Pt>>,
    /// Bounding size; everything lies in `0..w`, `0..h`.
    pub size: (f32, f32),
}

/// Lay out `g` whose nodes measure `sizes[k]` (w, h).
pub fn place(g: &Graph, sizes: &[(f32, f32)], sp: Spacing) -> Placed {
    let n = g.nodes.len();
    let horiz = matches!(g.dir, Dir::Right | Dir::Left);
    // Work top-down; a horizontal chart is a transposed vertical one.
    let mut sz: Vec<(f32, f32)> = sizes.iter().map(|&(w, h)| if horiz { (h, w) } else { (w, h) }).collect();
    let m = g.edges.len();

    // Break cycles: DFS back edges point up.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, e) in g.edges.iter().enumerate() {
        if e.from != e.to {
            out[e.from].push(k);
        }
    }
    let mut rev = vec![false; m];
    let mut state = vec![0u8; n];
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        let mut stack = vec![(s, 0usize)];
        state[s] = 1;
        while let Some(top) = stack.last_mut() {
            let v = top.0;
            if let Some(&k) = out[v].get(top.1) {
                top.1 += 1;
                let w = g.edges[k].to;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => rev[k] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    let oriented = |k: usize| {
        let e = &g.edges[k];
        if rev[k] { (e.to, e.from) } else { (e.from, e.to) }
    };

    // Longest-path ranks (Kahn order over the now acyclic edges).
    let mut rank = vec![0usize; n];
    let mut indeg = vec![0usize; n];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); n];
    for k in (0..m).filter(|&k| g.edges[k].from != g.edges[k].to) {
        let (u, v) = oriented(k);
        down[u].push(v);
        indeg[v] += 1;
    }
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut qi = 0;
    while qi < queue.len() {
        let u = queue[qi];
        qi += 1;
        for &v in &down[u] {
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }

    // Virtual graph: real nodes, then a dummy per rank crossed by a long edge.
    let ranks = rank.iter().max().map_or(0, |r| r + 1);
    let mut vrank = rank.clone();
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(m);
    for k in 0..m {
        let (u, v) = oriented(k);
        if u == v {
            chains.push(vec![u]);
            continue;
        }
        let mut c = vec![u];
        for r in rank[u] + 1..rank[v] {
            vrank.push(r);
            sz.push((0.0, 0.0));
            c.push(vrank.len() - 1);
        }
        c.push(v);
        chains.push(c);
    }
    let nv = vrank.len();
    let mut up_n: Vec<Vec<usize>> = vec![Vec::new(); nv];
    let mut down_n: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for c in &chains {
        for w in c.windows(2) {
            down_n[w[0]].push(w[1]);
            up_n[w[1]].push(w[0]);
        }
    }
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    // Initial order: discovery order (Kahn queue), then dummies.
    for v in queue.iter().copied().chain(n..nv) {
        layers[vrank[v]].push(v);
    }
    let mut pos = vec![0.0f32; nv];
    let index = |layers: &[Vec<usize>], pos: &mut [f32]| {
        for l in layers {
            for (i, &v) in l.iter().enumerate() {
                pos[v] = i as f32;
            }
        }
    };
    index(&layers, &mut pos);
    let mean = |vs: &[usize], at: &[f32]| (!vs.is_empty()).then(|| vs.iter().map(|&u| at[u]).sum::<f32>() / vs.len() as f32);
    for _ in 0..4 {
        for r in 1..ranks {
            let key: Vec<f32> = (0..nv).map(|v| mean(&up_n[v], &pos).unwrap_or(pos[v])).collect();
            layers[r].sort_by(|&a, &b| key[a].total_cmp(&key[b]));
            index(&layers, &mut pos);
        }
        for r in (0..ranks.saturating_sub(1)).rev() {
            let key: Vec<f32> = (0..nv).map(|v| mean(&down_n[v], &pos).unwrap_or(pos[v])).collect();
            layers[r].sort_by(|&a, &b| key[a].total_cmp(&key[b]));
            index(&layers, &mut pos);
        }
    }

    // x: pack, then pull toward neighbours keeping the separation (average of a left-to-right and
    // a right-to-left placement, both feasible, so their mean is too).
    let sep = |a: usize, b: usize| (sz[a].0 + sz[b].0) / 2.0 + sp.node_gap;
    let mut x = vec![0.0f32; nv];
    for l in &layers {
        for i in 1..l.len() {
            x[l[i]] = x[l[i - 1]] + sep(l[i - 1], l[i]);
        }
    }
    for it in 0..8 {
        let order: Vec<usize> = if it % 2 == 0 { (0..ranks).collect() } else { (0..ranks).rev().collect() };
        for r in order {
            let l = &layers[r];
            let want: Vec<f32> = l
                .iter()
                .map(|&v| {
                    let nb: Vec<usize> = up_n[v].iter().chain(&down_n[v]).copied().collect();
                    mean(&nb, &x).unwrap_or(x[v])
                })
                .collect();
            let k = l.len();
            let mut fwd = want.clone();
            for i in 1..k {
                fwd[i] = fwd[i].max(fwd[i - 1] + sep(l[i - 1], l[i]));
            }
            let mut bwd = want;
            for i in (0..k.saturating_sub(1)).rev() {
                bwd[i] = bwd[i].min(bwd[i + 1] - sep(l[i], l[i + 1]));
            }
            for i in 0..k {
                x[l[i]] = (fwd[i] + bwd[i]) / 2.0;
            }
        }
    }
    let left = (0..nv).map(|v| x[v] - sz[v].0 / 2.0).fold(f32::INFINITY, f32::min);
    let right = (0..nv).map(|v| x[v] + sz[v].0 / 2.0).fold(f32::NEG_INFINITY, f32::max);
    for v in x.iter_mut() {
        *v -= left;
    }
    let width = if n == 0 { 0.0 } else { right - left };

    // y: each rank as tall as its tallest node.
    let mut rank_h = vec![0.0f32; ranks];
    for v in 0..n {
        rank_h[rank[v]] = rank_h[rank[v]].max(sz[v].1);
    }
    let mut rank_y = vec![0.0f32; ranks];
    let mut y0 = 0.0;
    for r in 0..ranks {
        rank_y[r] = y0 + rank_h[r] / 2.0;
        y0 += rank_h[r] + sp.rank_gap;
    }
    let height = (y0 - sp.rank_gap).max(0.0);
    let at = |v: usize| (x[v], rank_y[vrank[v]]);

    let mut edges = Vec::with_capacity(m);
    let mut extra_w: f32 = 0.0;
    for (k, c) in chains.iter().enumerate() {
        if c.len() == 1 {
            // Self loop: a small hook on the node's right side.
            let ((cx, cy), (w, h)) = (at(c[0]), sz[c[0]]);
            let (r, d) = (w / 2.0, (h / 4.0).max(1.0));
            let hook = sp.node_gap * 0.6;
            extra_w = extra_w.max(cx + r + hook - width);
            edges.push(vec![(cx + r, cy - d), (cx + r + hook, cy - d), (cx + r + hook, cy + d), (cx + r, cy + d)]);
            continue;
        }
        let mut pts: Vec<Pt> = c.iter().map(|&v| at(v)).collect();
        let (a, b) = (c[0], c[c.len() - 1]);
        let last = pts.len() - 1;
        pts[0] = clip(pts[0], sz[a], g.nodes[a].shape, pts[1]);
        pts[last] = clip(pts[last], sz[b], g.nodes[b].shape, pts[last - 1]);
        if rev[k] {
            pts.reverse();
        }
        edges.push(pts);
    }

    let (w, h) = (width + extra_w.max(0.0), height);
    let tf = |(px, py): Pt| -> Pt {
        let (px, py) = if horiz { (py, px) } else { (px, py) };
        match g.dir {
            Dir::Down | Dir::Right => (px, py),
            Dir::Up => (px, h - py),
            Dir::Left => (h - px, py),
        }
    };
    let centers = (0..n).map(|v| tf(at(v))).collect();
    let edges = edges.into_iter().map(|e| e.into_iter().map(tf).collect()).collect();
    Placed { centers, edges, size: if horiz { (h, w) } else { (w, h) } }
}

/// Where the segment from centre `c` (node of size `s`) toward `p` leaves the node's outline.
pub fn clip(c: Pt, s: (f32, f32), shape: Shape, p: Pt) -> Pt {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    let (hw, hh) = (s.0 / 2.0, s.1 / 2.0);
    if hw <= 0.0 || hh <= 0.0 || (dx == 0.0 && dy == 0.0) {
        return c;
    }
    let t = match shape {
        Shape::Diamond => 1.0 / (dx.abs() / hw + dy.abs() / hh),
        Shape::Circle => 1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt(),
        _ => (hw / dx.abs()).min(hh / dy.abs()),
    };
    let t = t.min(1.0);
    (c.0 + dx * t, c.1 + dy * t)
}

#[cfg(test)]
mod tests {
    use super::super::mermaid::parse;
    use super::*;

    const SP: Spacing = Spacing { node_gap: 4.0, rank_gap: 8.0 };

    fn boxes(p: &Placed, sizes: &[(f32, f32)]) -> Vec<(f32, f32, f32, f32)> {
        p.centers.iter().zip(sizes).map(|(&(x, y), &(w, h))| (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0)).collect()
    }

    fn assert_no_overlap(b: &[(f32, f32, f32, f32)]) {
        for i in 0..b.len() {
            for j in i + 1..b.len() {
                let (p, q) = (b[i], b[j]);
                let apart = p.2 <= q.0 + 1e-3 || q.2 <= p.0 + 1e-3 || p.3 <= q.1 + 1e-3 || q.3 <= p.1 + 1e-3;
                assert!(apart, "nodes {i} and {j} overlap: {p:?} {q:?}");
            }
        }
    }

    #[test]
    fn ranks_follow_edges_and_nothing_overlaps() {
        let g = parse("graph TD\nA-->B\nA-->C\nB-->D\nC-->D\nA-->D").unwrap();
        let sizes = vec![(20.0, 8.0); 4];
        let p = place(&g, &sizes, SP);
        let y: Vec<f32> = p.centers.iter().map(|c| c.1).collect();
        assert!(y[0] < y[1] && y[1] == y[2] && y[2] < y[3]);
        assert_no_overlap(&boxes(&p, &sizes));
        // A→D spans two ranks: it bends through a dummy, so it has 3 points.
        assert_eq!(p.edges[4].len(), 3);
        for &(x, y) in p.edges.iter().flatten() {
            assert!((-1e-3..=p.size.0 + 1e-3).contains(&x) && (-1e-3..=p.size.1 + 1e-3).contains(&y));
        }
    }

    #[test]
    fn cycles_and_self_loops_still_lay_out() {
        let g = parse("graph LR\na-->b-->c-->a\nc-->c").unwrap();
        let sizes = vec![(10.0, 6.0); 3];
        let p = place(&g, &sizes, SP);
        assert_eq!(p.edges.len(), 4);
        let x: Vec<f32> = p.centers.iter().map(|c| c.0).collect();
        assert!(x[0] < x[1] && x[1] < x[2], "LR goes left to right: {x:?}");
        assert_no_overlap(&boxes(&p, &sizes));
        // The back edge c→a still starts at c and ends at a.
        let e = &p.edges[2];
        assert!(e[0].0 > e[e.len() - 1].0);
    }

    #[test]
    fn edges_end_on_outlines() {
        let g = parse("graph TD\nA{x}-->B((y))").unwrap();
        let sizes = [(16.0, 10.0), (10.0, 10.0)];
        let p = place(&g, &sizes, SP);
        let e = &p.edges[0];
        assert!((e[0].1 - (p.centers[0].1 + 5.0)).abs() < 1e-3, "leaves the diamond's bottom tip");
        assert!((e[1].1 - (p.centers[1].1 - 5.0)).abs() < 1e-3, "enters the circle's top");
    }

    #[test]
    fn bottom_up_flips() {
        let g = parse("graph BT\nA-->B").unwrap();
        let p = place(&g, &[(10.0, 6.0); 2], SP);
        assert!(p.centers[0].1 > p.centers[1].1);
    }
}
