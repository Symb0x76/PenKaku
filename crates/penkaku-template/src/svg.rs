//! A template as one SVG file: the paper drawn at real size (ruling, cell borders, hints, the
//! scanned background as an embedded PNG), so any image viewer shows it, plus the template itself
//! as TOML in `<metadata id="penkaku-template">`, so penkaku reads it back exactly.
//!
//! Page backgrounds live in a folder of PNGs while the app runs (`Background.file`); writing
//! embeds them, reading puts them back in that folder (named by content hash).

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use penkaku_core::font::Font;
use penkaku_core::layout::{self, PageBox, PageParams};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::template::{Template, TemplateKind};

const META_ID: &str = "penkaku-template";
/// Written before the rename to PenKaku; still read.
const LEGACY_META_ID: &str = "chixie-template";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn n(v: f32) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A ruling line in paper mm: ((x0, y0), (x1, y1), dashed).
type Line = ((f32, f32), (f32, f32), bool);

/// Ruling of a non-table template.
fn ruling_lines(t: &Template) -> Vec<Line> {
    let (l, top, r, b) = t.grid_margins();
    let params = PageParams { page: Some(PageBox { width: (t.paper.0 - l - r).max(1.0), height: (t.paper.1 - top - b).max(1.0) }), ruling: t.ruling(), ..Default::default() };
    let doc = layout::layout_ids(&[], &[], &Font::default(), &params);
    let Some(p) = doc.pages.first() else { return Vec::new() };
    p.guides.iter().map(|g| ((l + g.a.0, top - g.a.1), (l + g.b.0, top - g.b.1), g.dashed)).collect()
}

/// The template as SVG. `assets` is the folder holding page background PNGs.
pub fn write(t: &Template, assets: &Path) -> String {
    let (w, h) = t.paper;
    let pages = match &t.kind {
        TemplateKind::Table(tt) => tt.pages.len().max(1),
        _ => 1,
    };
    let total = h * pages as f32;
    let kind = match t.kind {
        TemplateKind::Blank => "blank",
        TemplateKind::Ruled { .. } => "ruled",
        TemplateKind::Grid { .. } => "grid",
        TemplateKind::Table(_) => "table",
    };
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" data-penkaku-template=\"1\" data-kind=\"{kind}\" data-name=\"{}\">\n",
        n(w),
        n(total),
        n(w),
        n(total),
        esc(&t.name)
    );
    s += &format!("<title>{}</title>\n", esc(&t.name));
    let toml = toml::to_string_pretty(t).unwrap_or_default().replace("]]>", "]]]]><![CDATA[>");
    s += &format!("<metadata id=\"{META_ID}\"><![CDATA[\n{toml}]]></metadata>\n");
    for k in 0..pages {
        s += &format!("<g id=\"page-{}\" transform=\"translate(0 {})\">\n", k + 1, n(h * k as f32));
        s += &format!("<rect width=\"{}\" height=\"{}\" fill=\"#fff\" stroke=\"#ccc\" stroke-width=\"0.3\"/>\n", n(w), n(h));
        match &t.kind {
            TemplateKind::Table(tt) => {
                let page = &tt.pages[k];
                if let Some(bg) = &page.background
                    && let Ok(png) = std::fs::read(assets.join(&bg.file))
                {
                    s += &format!(
                        "<image data-page=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" opacity=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>\n",
                        k + 1,
                        n(bg.x),
                        n(bg.y),
                        n(bg.w),
                        n(bg.h),
                        n(bg.opacity),
                        B64.encode(png)
                    );
                }
                s += "<g fill=\"none\" stroke=\"#000\" stroke-width=\"0.3\">\n";
                for tb in &page.tables {
                    for c in &tb.cells {
                        let (x, y, cw, ch) = tb.rect(c.span);
                        s += &format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n", n(x), n(y), n(cw), n(ch));
                    }
                }
                s += "</g>\n<g font-size=\"2.6\" fill=\"#888\">\n";
                for tb in &page.tables {
                    for c in tb.cells.iter().filter(|c| !c.hint.is_empty()) {
                        let (x, y, _, _) = tb.rect(c.span);
                        s += &format!("<text x=\"{}\" y=\"{}\">{}</text>\n", n(x + 0.8), n(y + 3.0), esc(&c.hint));
                    }
                }
                s += "</g>\n";
            }
            _ => {
                s += "<g stroke=\"#8ab\" stroke-width=\"0.2\">\n";
                for (a, b, dashed) in ruling_lines(t) {
                    let dash = if dashed { " stroke-dasharray=\"1.2 1.2\"" } else { "" };
                    s += &format!("<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{dash}/>\n", n(a.0), n(a.1), n(b.0), n(b.1));
                }
                s += "</g>\n";
            }
        }
        s += "</g>\n";
    }
    s += "</svg>\n";
    s
}

/// FNV-1a: a stable name for a background's content.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Read a template SVG. Embedded backgrounds are saved into `assets` and linked by file name.
pub fn read(svg: &str, assets: &Path) -> Result<Template, String> {
    let mut r = Reader::from_str(svg);
    let (mut in_meta, mut meta) = (false, String::new());
    let mut images: Vec<(usize, Vec<u8>)> = Vec::new();
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if e.local_name().as_ref() == b"metadata" => {
                in_meta = e.attributes().flatten().any(|a| a.key.as_ref() == b"id" && [META_ID, LEGACY_META_ID].iter().any(|id| a.value.as_ref() == id.as_bytes()));
            }
            Event::End(e) if e.local_name().as_ref() == b"metadata" => in_meta = false,
            Event::CData(c) if in_meta => meta += &String::from_utf8_lossy(&c),
            Event::Text(t) if in_meta => meta += &t.decode().map_err(|e| e.to_string())?,
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"image" => {
                let mut page = None;
                let mut data = None;
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        b"data-page" => page = String::from_utf8_lossy(&a.value).parse::<usize>().ok(),
                        b"href" | b"xlink:href" => {
                            let v = String::from_utf8_lossy(&a.value).to_string();
                            data = v.strip_prefix("data:image/png;base64,").and_then(|b| B64.decode(b.trim()).ok());
                        }
                        _ => {}
                    }
                }
                if let (Some(p), Some(d)) = (page, data) {
                    images.push((p.saturating_sub(1), d));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if meta.trim().is_empty() {
        return Err("不是 PenKaku 模板（缺少模板数据）".into());
    }
    let mut t: Template = toml::from_str(&meta).map_err(|e| e.to_string())?;
    if let TemplateKind::Table(tt) = &mut t.kind {
        for (k, png) in images {
            if let Some(bg) = tt.pages.get_mut(k).and_then(|p| p.background.as_mut()) {
                let file = format!("bg-{:016x}.png", fnv(&png));
                let path = assets.join(&file);
                if !path.exists() {
                    std::fs::create_dir_all(assets).map_err(|e| e.to_string())?;
                    std::fs::write(&path, &png).map_err(|e| e.to_string())?;
                }
                bg.file = file;
            }
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{Background, ColWidth, DataSource};
    use crate::template::A4;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("penkaku-svg-test-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn table_template(assets: &Path) -> Template {
        let mut t = Template::table("体温 & 记录 <表>", A4);
        t.tags = vec!["教育学习".into()];
        t.favourite = true;
        if let TemplateKind::Table(tt) = &mut t.kind {
            tt.insert_table(0, 3, 3, ColWidth::Fixed(40.0), A4.0, 20.0).unwrap();
            let tb = &mut tt.pages[0].tables[0];
            tb.merge(&[0, 1, 2]).unwrap();
            tb.cells[0].hint = "姓名".into();
            tb.cells[1].data = DataSource::RandomNumber { min: 36.0, max: 37.5, decimals: 1 };
            let png = {
                let img = image::GrayImage::from_fn(30, 20, |x, y| image::Luma([((x * 7 + y * 3) % 255) as u8]));
                let mut b = std::io::Cursor::new(Vec::new());
                img.write_to(&mut b, image::ImageFormat::Png).unwrap();
                b.into_inner()
            };
            std::fs::write(assets.join("scan.png"), &png).unwrap();
            tt.pages[0].background = Some(Background { file: "scan.png".into(), x: 10.0, y: 15.0, w: 150.0, h: 100.0, opacity: 0.4 });
            tt.add_page();
        }
        t
    }

    #[test]
    fn templates_saved_before_the_rename_still_read() {
        let a = dir("legacy");
        let t = Template::ruled("信纸", (160.0, 220.0), (1.5, 1.0, 1.5, 1.0), 22);
        let svg = write(&t, &a).replace(META_ID, LEGACY_META_ID).replace("data-penkaku-template", "data-chixie-template");
        assert_eq!(read(&svg, &a).unwrap(), t);
    }

    #[test]
    fn every_kind_round_trips() {
        let a = dir("kinds-a");
        for t in [
            Template::blank("空白", A4),
            Template::ruled("信纸", (160.0, 220.0), (1.5, 1.0, 1.5, 1.0), 22),
            Template::grid("作文格", (190.0, 255.0), 15.0, 20.0, 20, 20, (8.0, 8.0), (0.0, 2.9)),
            table_template(&a),
        ] {
            let svg = write(&t, &a);
            let b = dir("kinds-b");
            let back = read(&svg, &b).unwrap();
            if let (TemplateKind::Table(x), TemplateKind::Table(y)) = (&t.kind, &back.kind) {
                // The background came back as a new file with the same bytes.
                let (bx, by) = (x.pages[0].background.as_ref().unwrap(), y.pages[0].background.as_ref().unwrap());
                assert_eq!(std::fs::read(a.join(&bx.file)).unwrap(), std::fs::read(b.join(&by.file)).unwrap());
                let mut same = back.clone();
                if let TemplateKind::Table(z) = &mut same.kind {
                    z.pages[0].background.as_mut().unwrap().file = bx.file.clone();
                }
                assert_eq!(same, t);
            } else {
                assert_eq!(back, t, "{}", t.name);
            }
        }
    }

    #[test]
    fn the_svg_is_a_real_image() {
        let a = dir("image");
        let t = table_template(&a);
        let svg = write(&t, &a);
        assert!(svg.contains("width=\"210mm\" height=\"594mm\""), "two A4 pages stacked");
        assert!(svg.contains("data-penkaku-template=\"1\" data-kind=\"table\""));
        assert!(svg.contains("href=\"data:image/png;base64,"));
        assert!(svg.contains("体温 &amp; 记录 &lt;表&gt;"));
        // Well-formed: every event parses.
        let mut r = Reader::from_str(&svg);
        while r.read_event().unwrap() != Event::Eof {}
        let ruled = write(&Template::ruled("r", A4, (20.0, 20.0, 20.0, 20.0), 10), &a);
        assert_eq!(ruled.matches("<line").count(), 11, "10 rows = 11 rules");
    }

    #[test]
    fn hand_edited_files_still_read() {
        let a = dir("edited");
        let svg = write(&Template::grid("g", (190.0, 255.0), 15.0, 20.0, 5, 5, (8.0, 8.0), (0.0, 2.9)), &a);
        // An editor added attributes and an element, and wrote the TOML as plain text.
        let edited = svg.replace("<svg ", "<svg inkscape:version=\"1.3\" ").replace("</svg>", "<circle r=\"3\"/></svg>").replace("<![CDATA[", "").replace("]]></metadata>", "</metadata>");
        assert_eq!(read(&edited, &a).unwrap().name, "g");
        assert!(read("<svg xmlns=\"http://www.w3.org/2000/svg\"/>", &a).is_err());
    }
}
