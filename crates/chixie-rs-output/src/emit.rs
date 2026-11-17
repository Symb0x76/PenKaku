//! `Document` → Grbl G-code for a pen plotter.
//!
//! Each stroke: travel (`G0`) to its start, pen down (+ optional dwell), write moves (`G1`), pen
//! up. The page starts with a pen-up. Feeds are always explicit: the ChiXie3 pen literals carry no
//! `F` and rely on the modal feed (see docs/protocol.md).
//!
//! 多色绘制: a page written with several pens is cut into colour runs. A manual changer gets a
//! pen stop (line + colour) at each run start; the device pauses there. An automatic changer gets
//! its put-back / pick-up macros written in front of the run's first stroke instead.

use std::fmt::Write;

use chixie_rs_core::layout::{Document, Page};
use chixie_rs_core::profile::{DeviceProfile, DeviceType, PenChange, PenSlot, Rgb, num};

use crate::plan::Written;

/// G-code body for one page. The pen starts and ends up.
pub fn emit(page: &Page, d: &DeviceProfile) -> String {
    emit_mapped(page, d).text
}

/// A page body plus its resume map.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageCode {
    pub text: String,
    /// Line index (0-based, into `text.lines()`) of each stroke's travel `G0`. Line 0 is the
    /// leading pen-up; stroke `k` runs from `stroke_lines[k]` up to the next entry.
    pub stroke_lines: Vec<usize>,
    /// 多色绘制 with a manual changer: (line, colour) where each colour run starts; the device
    /// asks for that pen there unless it is already in the holder. Empty when single-colour.
    pub pens: Vec<(usize, Rgb)>,
}

impl PageCode {
    /// The body restarted at stroke `k`: the leading pen-up, then everything from that stroke's
    /// travel on, byte-identical to the uninterrupted tail, with its own resume map (strokes
    /// renumbered from 0). `k >= strokes` gives just the pen-up.
    pub fn from_stroke(&self, k: usize) -> PageCode {
        let mut text = String::new();
        if let Some(first) = self.text.lines().next() {
            text.push_str(first);
            text.push('\n');
        }
        let mut stroke_lines = Vec::new();
        if let Some(&start) = self.stroke_lines.get(k) {
            for l in self.text.lines().skip(start) {
                text.push_str(l);
                text.push('\n');
            }
            stroke_lines = self.stroke_lines[k..].iter().map(|&s| s - start + 1).collect();
        }
        // The restarted body opens with the colour of the run stroke `k` is in.
        let start = self.stroke_lines.get(k).copied().unwrap_or(usize::MAX);
        let mut pens: Vec<(usize, Rgb)> = self.pens.iter().filter(|p| p.0 >= start).map(|&(l, c)| (l - start + 1, c)).collect();
        if let Some(&(_, c)) = self.pens.iter().rev().find(|p| p.0 < start)
            && pens.first().is_none_or(|p| p.0 != 1)
        {
            pens.insert(0, (1, c));
        }
        PageCode { text, stroke_lines, pens }
    }

    /// Stroke that line `line` belongs to (`None` for the leading pen-up).
    pub fn stroke_at(&self, line: usize) -> Option<usize> {
        self.stroke_lines.partition_point(|&s| s <= line).checked_sub(1)
    }
}

/// `emit` with the line index of every stroke start.
pub fn emit_mapped(page: &Page, d: &DeviceProfile) -> PageCode {
    emit_runs(page, &[], d)
}

/// A written page (see [`crate::plan::write`]) with its pen stops or changer macros.
pub fn emit_written(w: &Written, d: &DeviceProfile) -> PageCode {
    emit_runs(&w.page, &w.pens, d)
}

/// The automatic changer's macro in front of a run: put the held pen back, pick the next one.
/// `None` when the next colour has no slot (that run falls back to a manual stop).
fn changer_macro(slots: &[PenSlot], held: Option<Rgb>, next: Rgb) -> Option<String> {
    let pick = slots.iter().find(|s| s.colour == next)?;
    let drop = held.and_then(|h| slots.iter().find(|s| s.colour == h)).map_or("", |s| s.drop.as_str());
    let mut m = String::new();
    for l in drop.lines().chain(pick.pick.lines()).map(str::trim).filter(|l| !l.is_empty()) {
        m.push_str(l);
        m.push('\n');
    }
    Some(m)
}

type P = (f32, f32);

/// 笔锋: the stroke with a Z on every point of its last `len` mm, rising linearly from `z0` (pen
/// down) to `z1` at the end. A point is inserted where the tail starts. Points before the tail
/// carry no Z (the pen stays at its down position).
fn taper_points(s: &[P], len: f32, z0: f32, z1: f32) -> Vec<(P, Option<f32>)> {
    let total: f32 = s.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    if s.len() < 2 || len <= 0.0 || total <= 0.0 {
        return s.iter().map(|&p| (p, None)).collect();
    }
    let len = len.min(total);
    let start = total - len;
    let z_at = |d: f32| z0 + (z1 - z0) * ((d - start) / len).clamp(0.0, 1.0);
    let mut out = vec![(s[0], (start <= 0.0).then(|| z_at(0.0)))];
    let mut d = 0.0;
    for w in s.windows(2) {
        let seg = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if d < start && d + seg > start {
            let f = (start - d) / seg;
            out.push(((w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f), Some(z0)));
        }
        d += seg;
        out.push((w[1], (d >= start).then(|| z_at(d))));
    }
    out
}

/// `runs`: (stroke index, colour) where each colour run starts; empty = one pen, no changes.
fn emit_runs(page: &Page, runs: &[(usize, Rgb)], d: &DeviceProfile) -> PageCode {
    let slots: &[PenSlot] = match &d.pen_change {
        PenChange::Auto { slots } => slots,
        PenChange::Manual => &[],
    };
    let (mut pens, mut held) = (Vec::new(), None);
    // 笔锋 needs a Z axis that moves continuously: stepper lift only.
    let taper = d.tip_taper.filter(|_| d.device_type == DeviceType::StepperPenUp);
    let (up, down) = (d.pen_up_cmd(), d.pen_down_cmd());
    let dwell = (d.pause_after_down_s > 0.0).then(|| format!("G4 P{}", num(d.pause_after_down_s)));
    let mut g = String::new();
    let mut stroke_lines = Vec::with_capacity(page.strokes.len());
    let _ = writeln!(g, "{up} F{:.0}", d.travel_speed);
    // Lines are counted from the text (a custom pen command may span several).
    let (mut line, mut counted) = (0, 0);
    for (k, stroke) in page.strokes.iter().enumerate() {
        line += g[counted..].matches('\n').count();
        counted = g.len();
        // An empty stroke is just its pen-up line; it starts there. A pen change in front of a
        // stroke belongs to it, so resuming at the stroke replays the change.
        stroke_lines.push(line);
        if let Some(&(_, colour)) = runs.iter().find(|r| r.0 == k) {
            match changer_macro(slots, held, colour) {
                Some(m) => g.push_str(&m),
                None => pens.push((line, colour)),
            }
            held = Some(colour);
        }
        let improved = d.route.apply(stroke);
        let pts = improved.as_deref().unwrap_or(stroke);
        let tapered = taper.map(|t| taper_points(pts, t.len_mm, d.pen_down_z, t.z_end));
        let zs: Vec<(P, Option<f32>)> = match tapered {
            Some(v) => v,
            None => pts.iter().map(|&p| (p, None)).collect(),
        };
        for (i, &(p, z)) in zs.iter().enumerate() {
            let (mx, my) = if d.axis_map.is_identity() { p } else { d.axis_map.apply(p) };
            if i == 0 {
                let _ = writeln!(g, "G0 X{mx:.3} Y{my:.3}");
                let _ = writeln!(g, "{down} F{:.0}", d.travel_speed);
                if let Some(w) = &dwell {
                    let _ = writeln!(g, "{w}");
                }
            }
            match z {
                Some(z) => _ = writeln!(g, "G1 X{mx:.3} Y{my:.3} Z{} F{:.0}", num(z), d.write_speed),
                None => _ = writeln!(g, "G1 X{mx:.3} Y{my:.3} F{:.0}", d.write_speed),
            }
        }
        // Unconditional (even for an empty stroke) to stay byte-identical with the original.
        let _ = writeln!(g, "{up} F{:.0}", d.travel_speed);
    }
    PageCode { text: g, stroke_lines, pens }
}

/// A self-contained program for the SD card (上传SD卡书写): mm and absolute mode first, then the
/// page body, then pen up and the 写完位置 move that a streamed job sends at its end.
pub fn standalone(body: &str, d: &DeviceProfile) -> String {
    let mut g = String::from("G21\nG90\n");
    g.push_str(body);
    let _ = writeln!(g, "{} F{:.0}", d.pen_up_cmd(), d.travel_speed);
    if let Some((x, y)) = d.end_position.target(&d.axis_map) {
        let _ = writeln!(g, "G90G0 X{}Y{}", num(x), num(y));
    }
    g
}

/// All pages back to back, for single-sheet output (CLI). Multi-page jobs should stream
/// `emit` per page with a sheet change in between.
pub fn emit_document(doc: &Document, d: &DeviceProfile) -> String {
    let mut out = String::new();
    for (i, page) in doc.pages.iter().enumerate() {
        let body = emit(page, d);
        // Pages after the first skip their leading pen-up: the previous page ended pen-up.
        out.push_str(if i == 0 { &body } else { body.split_once('\n').map_or("", |(_, rest)| rest) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chixie_rs_core::profile::AxisMap;

    fn page() -> Page {
        Page { strokes: vec![vec![(10.0, 50.0), (10.0, 48.0)]], ..Default::default() }
    }

    #[test]
    fn emits_pen_cycle() {
        let out = emit(&page(), &DeviceProfile::default());
        assert_eq!(
            out,
            "G1G90 Z0 F8000\nG0 X10.000 Y50.000\nG1G90 Z2 F8000\nG1 X10.000 Y50.000 F3000\n\
             G1 X10.000 Y48.000 F3000\nG1G90 Z0 F8000\n"
        );
    }

    fn two_colour_page() -> Written {
        let s = |x: f32| vec![(x, 0.0), (x + 1.0, 0.0)];
        Written { page: Page { strokes: vec![s(0.0), s(2.0), s(4.0)], ..Default::default() }, pens: vec![(0, [0, 0, 0]), (2, [255, 0, 0])] }
    }

    #[test]
    fn tip_taper_lifts_along_the_tail_only() {
        use chixie_rs_core::profile::TipTaper;
        let line = Page { strokes: vec![vec![(0.0, 0.0), (4.0, 0.0)]], ..Default::default() };
        let d = DeviceProfile { tip_taper: Some(TipTaper { len_mm: 1.0, z_end: 0.5 }), ..Default::default() };
        let out = emit(&line, &d);
        let g1: Vec<&str> = out.lines().filter(|l| l.starts_with("G1 X")).collect();
        assert_eq!(g1, ["G1 X0.000 Y0.000 F3000", "G1 X3.000 Y0.000 Z2 F3000", "G1 X4.000 Y0.000 Z0.5 F3000"]);
        // Z only falls towards the pen-up height along the tail.
        let pts = taper_points(&[(0.0, 0.0), (1.0, 0.0), (1.5, 0.0), (2.0, 0.0)], 1.5, 2.0, 0.5);
        let zs: Vec<f32> = pts.iter().filter_map(|p| p.1).collect();
        assert!(zs.windows(2).all(|w| w[1] <= w[0]) && zs.first() == Some(&2.0) && zs.last() == Some(&0.5), "{pts:?}");
        // Off, or not a stepper: unchanged.
        assert_eq!(emit(&line, &DeviceProfile { device_type: DeviceType::ServoPenUp, ..d.clone() }), emit(&line, &DeviceProfile { device_type: DeviceType::ServoPenUp, ..Default::default() }));
    }

    #[test]
    fn standalone_program_sets_modes_and_parks() {
        let d = DeviceProfile::default();
        let g = standalone(&emit(&page(), &d), &d);
        assert!(g.starts_with("G21\nG90\nG1G90 Z0 F8000\nG0 X10.000"), "{g}");
        assert!(g.ends_with("G1G90 Z0 F8000\nG1G90 Z0 F8000\nG90G0 X0Y0\n"), "{g}");
        let stay = DeviceProfile { end_position: chixie_rs_core::profile::EndPosition::Stay, ..Default::default() };
        assert!(standalone("", &stay).ends_with("Z0 F8000\n"));
    }

    #[test]
    fn manual_changer_stops_at_each_run() {
        let pc = emit_written(&two_colour_page(), &DeviceProfile::default());
        assert_eq!(pc.pens, vec![(1, [0, 0, 0]), (pc.stroke_lines[2], [255, 0, 0])]);
        assert_eq!(pc.text, emit(&two_colour_page().page, &DeviceProfile::default()), "manual stops add no G-code");
        // Resuming inside the black run asks for black first; at the red run, for red.
        assert_eq!(pc.from_stroke(1).pens, vec![(1, [0, 0, 0]), (6, [255, 0, 0])]);
        assert_eq!(pc.from_stroke(2).pens, vec![(1, [255, 0, 0])]);
    }

    #[test]
    fn auto_changer_writes_macros_instead_of_stops() {
        use chixie_rs_core::profile::PenSlot;
        let slot = |c: Rgb, n: u8| PenSlot { colour: c, pick: format!("M800 P{n}"), drop: format!("M801 P{n}") };
        let d = DeviceProfile { pen_change: PenChange::Auto { slots: vec![slot([0, 0, 0], 1), slot([255, 0, 0], 2)] }, ..Default::default() };
        let pc = emit_written(&two_colour_page(), &d);
        assert!(pc.pens.is_empty());
        let lines: Vec<&str> = pc.text.lines().collect();
        assert_eq!(lines[pc.stroke_lines[0]], "M800 P1");
        assert_eq!(&lines[pc.stroke_lines[2]..pc.stroke_lines[2] + 3], ["M801 P1", "M800 P2", "G0 X4.000 Y0.000"]);
        // A colour without a slot falls back to a manual stop.
        let d1 = DeviceProfile { pen_change: PenChange::Auto { slots: vec![slot([0, 0, 0], 1)] }, ..Default::default() };
        assert_eq!(emit_written(&two_colour_page(), &d1).pens.len(), 1);
    }

    #[test]
    fn resume_map_restarts_with_identical_tail() {
        let page = Page { strokes: vec![vec![(1.0, 1.0), (2.0, 1.0)], vec![], vec![(3.0, 3.0)]], ..Default::default() };
        let d = DeviceProfile { pause_after_down_s: 0.5, ..Default::default() };
        let pc = emit_mapped(&page, &d);
        assert_eq!(pc.text, emit(&page, &d));
        let lines: Vec<&str> = pc.text.lines().collect();
        // pen-up, [G0, down, dwell, G1, G1, up], [up], [G0, down, dwell, G1, up]
        assert_eq!(pc.stroke_lines, vec![1, 7, 8]);
        assert!(lines[pc.stroke_lines[2]].starts_with("G0 X3.000"));
        assert_eq!(pc.from_stroke(0), pc);
        let tail = pc.from_stroke(1);
        assert_eq!(tail.text, format!("{}\n{}\n", lines[0], lines[7..].join("\n")));
        assert_eq!(tail.stroke_lines, vec![1, 2]);
        assert_eq!(pc.from_stroke(3).text, "G1G90 Z0 F8000\n");
        assert_eq!((pc.stroke_at(0), pc.stroke_at(1), pc.stroke_at(6), pc.stroke_at(7), pc.stroke_at(12)), (None, Some(0), Some(0), Some(1), Some(2)));
    }

    #[test]
    fn route_smoothing_keeps_the_resume_map_per_stroke() {
        use chixie_rs_core::path::{RouteImprove, RouteMode};
        let wiggly: Vec<(f32, f32)> = (0..=20).map(|i| (i as f32 * 0.5, if i % 2 == 0 { 0.0 } else { 0.01 })).collect();
        let page = Page { strokes: vec![wiggly.clone(), vec![(3.0, 3.0), (4.0, 3.0)]], ..Default::default() };
        let d = DeviceProfile { route: RouteImprove { mode: RouteMode::Simplify, value: 0.02 }, ..Default::default() };
        let pc = emit_mapped(&page, &d);
        // First stroke shrinks to its two endpoints: pen-up, [G0, down, G1, G1, up], [G0, …].
        assert_eq!(pc.stroke_lines, vec![1, 6]);
        assert!(pc.text.lines().nth(6).unwrap().starts_with("G0 X3.000"));
        assert!(pc.text.lines().count() < emit(&page, &DeviceProfile::default()).lines().count());
        assert_eq!(pc.from_stroke(1).text.lines().nth(1), Some("G0 X3.000 Y3.000"));
    }

    #[test]
    fn empty_page_is_just_pen_up() {
        assert_eq!(emit(&Page::default(), &DeviceProfile::default()), "G1G90 Z0 F8000\n");
    }

    #[test]
    fn z_heights_dwell_and_custom_commands() {
        let d = DeviceProfile { pen_up_z: 0.5, pen_down_z: 3.25, pause_after_down_s: 0.2, ..Default::default() };
        let out = emit(&page(), &d);
        assert!(out.starts_with("G1G90 Z0.5 F8000\nG0 X10.000 Y50.000\nG1G90 Z3.25 F8000\nG4 P0.2\nG1 X10.000"), "{out}");
        let c = DeviceProfile { device_type: DeviceType::CustomZ, pen_up: "M5".into(), pen_down: "M3 S90".into(), ..Default::default() };
        assert_eq!((c.pen_up_cmd(), c.pen_down_cmd()), ("M5".to_string(), "M3 S90".to_string()));
    }

    #[test]
    fn axis_map_swaps_and_inverts() {
        let d = DeviceProfile { axis_map: AxisMap { swap_xy: true, invert_x: true, invert_y: false }, ..Default::default() };
        let out = emit(&page(), &d);
        assert!(out.contains("G0 X-50.000 Y10.000"), "{out}");
    }
}
