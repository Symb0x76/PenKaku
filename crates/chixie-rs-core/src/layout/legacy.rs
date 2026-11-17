//! The original `writing::text_to_gcode`, kept verbatim as a test oracle for the split.
//! Delete once the split has been verified on hardware (plan M2: oracle goes, golden stays).

use std::fmt::Write;
use super::PageParams;
use crate::font::Font;
use crate::gcode::DeviceProfile;
pub fn text_to_gcode(text: &str, font: &Font, p: &PageParams, d: &DeviceProfile) -> String {
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
