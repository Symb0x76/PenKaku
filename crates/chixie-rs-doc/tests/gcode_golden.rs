//! Determinism: the same document + seed + font gives byte-identical G-code, before and after a
//! DOCX round trip (ids and seed survive the file, so the handwriting is unchanged).

use std::collections::HashMap;

use chixie_rs_core::font::{Font, Glyph};
use chixie_rs_core::profile::DeviceProfile;
use chixie_rs_output as gcode;
use chixie_rs_core::handwriting::HandwritingStyle;
use chixie_rs_core::layout::{self, PageBox, PageParams};
use chixie_rs_doc::{EditDoc, docx};

fn font() -> Font {
    let g = |advance: f32, s: &[(f32, f32)]| Glyph { advance, strokes: vec![s.to_vec(), vec![(0.5, 6.5), (6.5, 0.5)]] };
    let glyphs: HashMap<char, Glyph> = [
        ('你', g(7.0, &[(0.0, 0.0), (7.0, 7.0)])),
        ('好', g(7.0, &[(1.0, 0.0), (1.0, 7.0)])),
        ('写', g(7.0, &[(0.0, 3.0), (7.0, 3.0)])),
        ('a', g(4.0, &[(0.0, 2.0), (4.0, 7.0)])),
    ]
    .into_iter()
    .collect();
    Font { glyphs }
}

fn gcode_of(d: &EditDoc, seed: u64) -> String {
    let p = PageParams {
        mm_per_unit: 1.2,
        page: Some(PageBox { width: 60.0, height: 40.0 }),
        style: HandwritingStyle { enabled: true, seed, ..Default::default() },
        ..Default::default()
    };
    let laid = layout::layout_doc(d.chars(), d.ids(), d.document().paras(), &font(), &p);
    gcode::emit_document(&laid, &DeviceProfile::default())
}

#[test]
fn docx_round_trip_keeps_gcode_byte_identical() {
    let mut d = EditDoc::from_text("你好写a你好写a\n写好你a 你好\n你好写a你好写a你好写a");
    // Edits so ids are no longer 0..n.
    d.move_to(3, false);
    d.insert("好写");
    d.move_to(0, false);
    d.delete(false);
    // Paragraph formatting is part of the round trip too.
    d.move_to(20, false);
    d.set_align(layout::Align::Right);
    let before = gcode_of(&d, 77);
    let loaded = docx::read(&docx::write(d.document(), 77).unwrap()).unwrap();
    let after = gcode_of(&EditDoc::from_document(loaded.doc), loaded.seed.unwrap());
    assert!(before.lines().count() > 50, "a real job");
    assert_eq!(before, after);
    // And the seed matters (the comparison above is not trivially true).
    assert_ne!(before, gcode_of(&d, 78));
}
