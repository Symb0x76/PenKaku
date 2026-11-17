//! The original `writing::text_to_gcode`, kept verbatim as a test oracle for the layout split.
//! Delete once the split has been verified on hardware (plan M2: oracle goes, golden stays).

use std::fmt::Write;

use chixie_rs_core::font::Font;
use chixie_rs_core::layout::{PageParams, layout};
use chixie_rs_core::profile::DeviceProfile;
use chixie_rs_output::emit_document;

const FIXTURE: &[u8] = include_bytes!("../../chixie-rs-core/tests/fixtures/font_fixture.json");

fn text_to_gcode(text: &str, font: &Font, p: &PageParams, d: &DeviceProfile) -> String {
    let mut g = String::new();
    let _ = writeln!(g, "{} F{:.0}", d.pen_up, d.travel_speed);
    let (ox, oy) = p.origin;
    let mut cx = 0.0f32;
    let mut line_top = 0.0f32;
    for ch in text.chars() {
        match ch {
            '\n' => {
                cx = 0.0;
                line_top += p.line_height;
                continue;
            }
            ' ' => {
                cx += font.get(' ').map(|gl| gl.advance * p.mm_per_unit).unwrap_or(p.space_width);
                cx += p.char_spacing;
                continue;
            }
            _ => {}
        }
        let Some(glyph) = font.get(ch) else {
            cx += p.space_width + p.char_spacing;
            continue;
        };
        for stroke in &glyph.strokes {
            for (i, &(gx, gy)) in stroke.iter().enumerate() {
                let mx = ox + cx + gx * p.mm_per_unit;
                let my = oy - (line_top + gy * p.mm_per_unit);
                if i == 0 {
                    let _ = writeln!(g, "G0 X{mx:.3} Y{my:.3}");
                    let _ = writeln!(g, "{} F{:.0}", d.pen_down, d.travel_speed);
                    let _ = writeln!(g, "G1 X{mx:.3} Y{my:.3} F{:.0}", d.write_speed);
                } else {
                    let _ = writeln!(g, "G1 X{mx:.3} Y{my:.3} F{:.0}", d.write_speed);
                }
            }
            let _ = writeln!(g, "{} F{:.0}", d.pen_up, d.travel_speed);
        }
        cx += glyph.advance * p.mm_per_unit + p.char_spacing;
    }
    g
}

#[test]
fn matches_legacy_oracle_across_params() {
    let font = Font::from_json(FIXTURE).unwrap();
    let d = DeviceProfile::default();
    let texts = ["你好 A.\n好你", "", "  ", "A\n\n.你", "x你y好z", "好好好好好好好好好好好好"];
    let params = [
        PageParams::default(),
        PageParams { mm_per_unit: 3.0, origin: (0.0, 24.0), ..Default::default() },
        PageParams { mm_per_unit: 0.37, char_spacing: 0.13, origin: (12.5, 280.1), ..Default::default() },
        PageParams { mm_per_unit: 7.1, space_width: 2.2, line_height: 13.7, ..Default::default() },
    ];
    for t in texts {
        for p in &params {
            let new = emit_document(&layout(t, &font, p), &d);
            assert_eq!(new, text_to_gcode(t, &font, p, &d), "text {t:?} params {p:?}");
        }
    }
}

