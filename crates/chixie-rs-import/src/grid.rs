//! Ruling segments → tables. Segments that touch form a table; its grid is every distinct x of
//! its vertical lines × every distinct y of its horizontal ones; neighbouring grid squares belong
//! to the same cell when the line between them is (mostly) missing.

/// A horizontal ruling segment from `x0` to `x1` at `y` (page units, y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HSeg {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
}

/// A vertical ruling segment from `y0` to `y1` at `x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VSeg {
    pub y0: f32,
    pub y1: f32,
    pub x: f32,
}

/// A table found on a page: top-left corner, column widths, row heights (page units) and the
/// cells as grid spans `(r0, c0, r1, c1)` (end exclusive) in writing order.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedTable {
    pub x: f32,
    pub y: f32,
    pub cols: Vec<f32>,
    pub rows: Vec<f32>,
    pub cells: Vec<(u32, u32, u32, u32)>,
}

impl DetectedTable {
    pub fn width(&self) -> f32 {
        self.cols.iter().sum()
    }
    pub fn height(&self) -> f32 {
        self.rows.iter().sum()
    }
    /// Scale every length by `k` (px → mm).
    pub fn scaled(&self, k: f32) -> Self {
        Self { x: self.x * k, y: self.y * k, cols: self.cols.iter().map(|c| c * k).collect(), rows: self.rows.iter().map(|r| r * k).collect(), cells: self.cells.clone() }
    }
}

/// Positions within `tol` of each other, merged to their mean (sorted).
fn cluster(mut v: Vec<f32>, tol: f32) -> Vec<f32> {
    v.sort_by(f32::total_cmp);
    let mut out: Vec<(f32, u32)> = Vec::new();
    for x in v {
        match out.last_mut() {
            Some((sum, n)) if x - *sum / *n as f32 <= tol => {
                *sum += x;
                *n += 1;
            }
            _ => out.push((x, 1)),
        }
    }
    out.into_iter().map(|(s, n)| s / n as f32).collect()
}

/// Join collinear pieces: same line within `tol`, overlapping or with gaps ≤ `tol`.
fn merge_h(segs: &[HSeg], tol: f32) -> Vec<HSeg> {
    let mut s: Vec<HSeg> = segs.iter().map(|g| HSeg { x0: g.x0.min(g.x1), x1: g.x0.max(g.x1), y: g.y }).collect();
    s.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut out: Vec<HSeg> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let mut j = i;
        while j + 1 < s.len() && s[j + 1].y - s[i].y <= tol {
            j += 1;
        }
        let y = s[i..=j].iter().map(|g| g.y).sum::<f32>() / (j - i + 1) as f32;
        let mut row: Vec<HSeg> = s[i..=j].to_vec();
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut cur = HSeg { y, ..row[0] };
        for g in &row[1..] {
            if g.x0 <= cur.x1 + tol {
                cur.x1 = cur.x1.max(g.x1);
            } else {
                out.push(cur);
                cur = HSeg { y, ..*g };
            }
        }
        out.push(cur);
        i = j + 1;
    }
    out
}

fn merge_v(segs: &[VSeg], tol: f32) -> Vec<VSeg> {
    let as_h: Vec<HSeg> = segs.iter().map(|v| HSeg { x0: v.y0, x1: v.y1, y: v.x }).collect();
    merge_h(&as_h, tol).into_iter().map(|h| VSeg { y0: h.x0, y1: h.x1, x: h.y }).collect()
}

/// Length of `a..b` covered by the intervals.
fn covered(a: f32, b: f32, ivs: &[(f32, f32)]) -> f32 {
    let mut parts: Vec<(f32, f32)> = ivs.iter().map(|&(p, q)| (p.max(a), q.min(b))).filter(|(p, q)| q > p).collect();
    parts.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (mut len, mut end) = (0.0, a);
    for (p, q) in parts {
        if q > end {
            len += q - p.max(end);
            end = q;
        }
    }
    len
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut k = i;
    while parent[k] != r {
        let next = parent[k];
        parent[k] = r;
        k = next;
    }
    r
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// Tables from ruling segments. `tol` (page units) is how far apart two pieces of one line may be;
/// about one line width. Tables come top to bottom, then left to right.
pub fn build(h: &[HSeg], v: &[VSeg], tol: f32) -> Vec<DetectedTable> {
    let h = merge_h(h, tol);
    let v = merge_v(v, tol);
    // Connected segments: an H and a V touch when they cross (ends may fall short by `tol`).
    let n = h.len() + v.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for (i, a) in h.iter().enumerate() {
        for (j, b) in v.iter().enumerate() {
            if b.x >= a.x0 - tol && b.x <= a.x1 + tol && a.y >= b.y0 - tol && a.y <= b.y1 + tol {
                union(&mut parent, i, h.len() + j);
            }
        }
    }
    let mut groups: Vec<(Vec<HSeg>, Vec<VSeg>)> = Vec::new();
    let mut root_of: Vec<(usize, usize)> = Vec::new();
    for k in 0..n {
        let r = find(&mut parent, k);
        let g = match root_of.iter().find(|(root, _)| *root == r) {
            Some(&(_, g)) => g,
            None => {
                groups.push((Vec::new(), Vec::new()));
                root_of.push((r, groups.len() - 1));
                groups.len() - 1
            }
        };
        if k < h.len() {
            groups[g].0.push(h[k]);
        } else {
            groups[g].1.push(v[k - h.len()]);
        }
    }
    let mut tables: Vec<DetectedTable> = groups.into_iter().filter(|(hs, vs)| hs.len() >= 2 && vs.len() >= 2).filter_map(|(hs, vs)| table(&hs, &vs, tol)).collect();
    tables.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap_or(std::cmp::Ordering::Equal));
    tables
}

fn table(hs: &[HSeg], vs: &[VSeg], tol: f32) -> Option<DetectedTable> {
    let xs = cluster(vs.iter().map(|v| v.x).collect(), tol);
    let ys = cluster(hs.iter().map(|h| h.y).collect(), tol);
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    let (nr, nc) = (ys.len() - 1, xs.len() - 1);
    // Segments on each grid line (by nearest line).
    let near = |p: f32, lines: &[f32]| lines.iter().position(|&l| (l - p).abs() <= tol);
    let mut on_x: Vec<Vec<(f32, f32)>> = vec![Vec::new(); xs.len()];
    for v in vs {
        if let Some(k) = near(v.x, &xs) {
            on_x[k].push((v.y0, v.y1));
        }
    }
    let mut on_y: Vec<Vec<(f32, f32)>> = vec![Vec::new(); ys.len()];
    for h in hs {
        if let Some(k) = near(h.y, &ys) {
            on_y[k].push((h.x0, h.x1));
        }
    }
    let present = |a: f32, b: f32, ivs: &[(f32, f32)]| covered(a, b, ivs) >= 0.5 * (b - a);
    // Squares that a missing line joins.
    let sq = |r: usize, c: usize| r * nc + c;
    let mut parent: Vec<usize> = (0..nr * nc).collect();
    for r in 0..nr {
        for c in 0..nc {
            if c + 1 < nc && !present(ys[r], ys[r + 1], &on_x[c + 1]) {
                union(&mut parent, sq(r, c), sq(r, c + 1));
            }
            if r + 1 < nr && !present(xs[c], xs[c + 1], &on_y[r + 1]) {
                union(&mut parent, sq(r, c), sq(r + 1, c));
            }
        }
    }
    // Regions → cells; a region that is not a rectangle falls back to its squares.
    let mut regions: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
    for r in 0..nr {
        for c in 0..nc {
            let root = find(&mut parent, sq(r, c));
            match regions.iter_mut().find(|(k, _)| *k == root) {
                Some((_, v)) => v.push((r, c)),
                None => regions.push((root, vec![(r, c)])),
            }
        }
    }
    let mut cells: Vec<(u32, u32, u32, u32)> = Vec::new();
    for (_, sqs) in regions {
        let (r0, c0) = (sqs.iter().map(|s| s.0).min()?, sqs.iter().map(|s| s.1).min()?);
        let (r1, c1) = (sqs.iter().map(|s| s.0).max()? + 1, sqs.iter().map(|s| s.1).max()? + 1);
        if (r1 - r0) * (c1 - c0) == sqs.len() {
            cells.push((r0 as u32, c0 as u32, r1 as u32, c1 as u32));
        } else {
            cells.extend(sqs.iter().map(|&(r, c)| (r as u32, c as u32, r as u32 + 1, c as u32 + 1)));
        }
    }
    cells.sort_unstable();
    Some(DetectedTable {
        x: xs[0],
        y: ys[0],
        cols: xs.windows(2).map(|w| w[1] - w[0]).collect(),
        rows: ys.windows(2).map(|w| w[1] - w[0]).collect(),
        cells,
    })
}

/// Segments of a grid, for tests: `xs` × `ys` lines with the cells' borders only (a merged
/// cell has no inner lines).
#[doc(hidden)]
pub fn segments_of(xs: &[f32], ys: &[f32], cells: &[(u32, u32, u32, u32)]) -> (Vec<HSeg>, Vec<VSeg>) {
    let (mut h, mut v) = (Vec::new(), Vec::new());
    for &(r0, c0, r1, c1) in cells {
        let (x0, x1, y0, y1) = (xs[c0 as usize], xs[c1 as usize], ys[r0 as usize], ys[r1 as usize]);
        h.push(HSeg { x0, x1, y: y0 });
        h.push(HSeg { x0, x1, y: y1 });
        v.push(VSeg { y0, y1, x: x0 });
        v.push(VSeg { y0, y1, x: x1 });
    }
    (h, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_cells(nr: u32, nc: u32) -> Vec<(u32, u32, u32, u32)> {
        (0..nr).flat_map(|r| (0..nc).map(move |c| (r, c, r + 1, c + 1))).collect()
    }

    #[test]
    fn plain_grid() {
        let (xs, ys) = ([10.0, 40.0, 70.0, 100.0], [20.0, 30.0, 40.0]);
        let cells = grid_cells(2, 3);
        let (h, v) = segments_of(&xs, &ys, &cells);
        let t = build(&h, &v, 0.5);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].x, t[0].y), (10.0, 20.0));
        assert_eq!(t[0].cols, vec![30.0; 3]);
        assert_eq!(t[0].rows, vec![10.0; 2]);
        assert_eq!(t[0].cells, cells);
    }

    #[test]
    fn merged_header_and_tall_cell() {
        let (xs, ys) = ([0.0, 20.0, 50.0, 80.0], [0.0, 8.0, 16.0, 24.0]);
        // Header across all columns; left column rows 1..3 merged.
        let cells = vec![(0, 0, 1, 3), (1, 0, 3, 1), (1, 1, 2, 2), (1, 2, 2, 3), (2, 1, 3, 2), (2, 2, 3, 3)];
        let (h, v) = segments_of(&xs, &ys, &cells);
        let t = build(&h, &v, 0.5);
        assert_eq!(t[0].cells, cells);
    }

    #[test]
    fn rough_lines_gaps_overshoot_and_two_tables() {
        // Table 1 drawn in pieces with small gaps, overshoots and jitter.
        let h = vec![
            HSeg { x0: -0.6, x1: 30.0, y: 0.2 },
            HSeg { x0: 30.3, x1: 60.4, y: -0.1 },
            HSeg { x0: 0.1, x1: 60.0, y: 10.3 },
            HSeg { x0: 0.0, x1: 59.8, y: 19.9 },
            // Table 2, further down.
            HSeg { x0: 0.0, x1: 40.0, y: 50.0 },
            HSeg { x0: 0.0, x1: 40.0, y: 60.0 },
        ];
        let v = vec![
            VSeg { y0: 0.0, y1: 20.0, x: 0.2 },
            VSeg { y0: -0.4, y1: 9.6, x: 30.1 },
            VSeg { y0: 10.2, y1: 20.3, x: 29.8 },
            VSeg { y0: 0.0, y1: 20.0, x: 60.1 },
            VSeg { y0: 50.0, y1: 60.0, x: 0.0 },
            VSeg { y0: 50.0, y1: 60.0, x: 40.0 },
        ];
        let t = build(&h, &v, 1.0);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].cells, grid_cells(2, 2));
        assert!((t[0].cols[0] - 30.0).abs() < 0.5 && (t[0].rows[1] - 9.7).abs() < 0.5);
        assert_eq!(t[1].cells, vec![(0, 0, 1, 1)]);
        assert_eq!(t[1].y, 50.0);
    }

    #[test]
    fn a_partial_line_still_splits_and_noise_is_ignored() {
        // The inner vertical covers only the bottom row: top row is one merged cell.
        let h = vec![HSeg { x0: 0.0, x1: 40.0, y: 0.0 }, HSeg { x0: 0.0, x1: 40.0, y: 10.0 }, HSeg { x0: 0.0, x1: 40.0, y: 20.0 }, HSeg { x0: 100.0, x1: 103.0, y: 5.0 }];
        let v = vec![VSeg { y0: 0.0, y1: 20.0, x: 0.0 }, VSeg { y0: 10.0, y1: 20.0, x: 20.0 }, VSeg { y0: 0.0, y1: 20.0, x: 40.0 }];
        let t = build(&h, &v, 0.5);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].cells, vec![(0, 0, 1, 2), (1, 0, 2, 1), (1, 1, 2, 2)]);
    }
}
