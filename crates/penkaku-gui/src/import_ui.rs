//! 文件 › 导入… (or a file dropped on the page): text from `.txt` `.docx` `.pdf` `.xlsx` into the
//! current document. When the page already has text, ChiXie3's question: 覆盖 / 追加 (and our
//! 在光标处插入). On a table form a workbook fills the cells (M7a mapping).

use std::path::{Path, PathBuf};

use penkaku_doc::import::{ImportMode, decode_text, docx_chars};
use penkaku_doc::model::Attrs;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::doc_view::DocView;
use crate::state::AppState;

/// What a file gives: text (with formatting when it has some), or sheets for a table form.
#[derive(Debug)]
pub enum ImportSource {
    Chars(Vec<(char, Option<Attrs>)>),
    Workbook(Vec<penkaku_template::xlsx::Sheet>),
}

/// Read `path` for importing; `table`: the page is a table form. Errors are messages to show.
pub fn read_import(path: &Path, table: bool) -> Result<ImportSource, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let read = || std::fs::read(path).map_err(|e| format!("读取文件失败：{e}"));
    let plain = |s: String| ImportSource::Chars(s.chars().map(|c| (c, None)).collect());
    match ext.as_str() {
        "txt" | "text" | "md" | "csv" => Ok(plain(decode_text(&read()?))),
        "docx" => Ok(ImportSource::Chars(docx_chars(&read()?)?.into_iter().map(|(c, a)| (c, Some(a))).collect())),
        "pdf" => {
            let pages = penkaku_import::pdf::text(&read()?)?;
            if pages.iter().all(|p| p.trim().is_empty()) {
                return Err("该 PDF 没有可提取的文字（可能是扫描件），请复制文字后粘贴".into());
            }
            Ok(plain(pages.join("\n")))
        }
        "xlsx" | "xlsm" | "xls" | "ods" => {
            let sheets = penkaku_template::xlsx::read_sheets(&read()?)?;
            if table {
                return Ok(ImportSource::Workbook(sheets));
            }
            // Notes: one line per row, cells joined by spaces.
            let rows: Vec<String> = sheets
                .iter()
                .flat_map(|s| s.iter().map(|r| r.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).collect::<Vec<_>>().join(" ")))
                .filter(|r| !r.is_empty())
                .collect();
            Ok(plain(rows.join("\n")))
        }
        "doc" => Err("不支持 .doc 文件，请在 Word / WPS 中另存为 .docx 后再导入".into()),
        _ => Err("不支持的文件格式（可导入 txt、docx、pdf、xlsx）".into()),
    }
}

/// Pick a file and import it.
pub fn pick_and_import(view: Entity<DocView>, window: &mut Window, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("导入（txt / docx / pdf / xlsx）".into()) });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = cx.update(|w, cx| import_path(view, path, w, cx));
        })
        .detach();
}

/// Import `path` into the document, asking how when it already has text.
pub fn import_path(view: Entity<DocView>, path: PathBuf, window: &mut Window, cx: &mut App) {
    let table = matches!(cx.global::<AppState>().active_template().map(|t| t.kind), Some(penkaku_template::TemplateKind::Table(_)));
    let chars = match read_import(&path, table) {
        Ok(ImportSource::Workbook(sheets)) => return crate::table_ui::import_workbook(view, &sheets, window, cx),
        Ok(ImportSource::Chars(c)) => c,
        Err(e) => return window.push_notification(Notification::warning(e), cx),
    };
    if chars.is_empty() {
        return window.push_notification(Notification::info("文件中没有文字"), cx);
    }
    if view.read(cx).doc().is_empty() {
        view.update(cx, |v, cx| v.import_chars(chars, ImportMode::Overwrite, cx));
        return;
    }
    window.open_dialog(cx, move |d, _w, _cx| {
        let pick = |mode: ImportMode| {
            let (view, chars) = (view.clone(), chars.clone());
            move |_: &ClickEvent, w: &mut Window, cx: &mut App| {
                w.close_dialog(cx);
                view.update(cx, |v, cx| v.import_chars(chars.clone(), mode, cx));
            }
        };
        d.title("导入")
            .w(px(380.))
            .child(v_flex().gap_2().child(div().text_sm().child("检测到页面已有内容，请问是覆盖还是追加？")))
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(Button::new("imp-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx)))
                    .child(Button::new("imp-insert").label("在光标处插入").on_click(pick(ImportMode::AtCaret)))
                    .child(Button::new("imp-append").label("追加").on_click(pick(ImportMode::Append)))
                    .child(Button::new("imp-over").primary().label("覆盖").on_click(pick(ImportMode::Overwrite))),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let d = std::env::temp_dir().join("penkaku-gui-test-import");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn text_of(s: ImportSource) -> String {
        match s {
            ImportSource::Chars(c) => c.into_iter().map(|(c, _)| c).collect(),
            ImportSource::Workbook(_) => panic!("workbook"),
        }
    }

    #[core::prelude::v1::test]
    fn each_format_reads_its_text() {
        // GB18030 text (你好).
        assert_eq!(text_of(read_import(&tmp("a.txt", &[0xC4, 0xE3, 0xBA, 0xC3]), false).unwrap()), "你好");
        let docx = penkaku_doc::docx::write(&penkaku_doc::Document::from_text("段一\n段二"), 1).unwrap();
        assert_eq!(text_of(read_import(&tmp("b.docx", &docx), false).unwrap()), "段一\n段二");
        let pdf = penkaku_import::synth::pdf_with_text(&["Page one", "Page two"]);
        assert_eq!(text_of(read_import(&tmp("c.pdf", &pdf), false).unwrap()), "Page one\nPage two");
        let scan = penkaku_import::synth::pdf_with_text(&[""]);
        assert!(read_import(&tmp("d.pdf", &scan), false).is_err());
        assert!(read_import(&tmp("e.doc", b"x"), false).unwrap_err().contains(".docx"));
    }

    #[core::prelude::v1::test]
    fn workbook_rows_become_lines_or_fill_a_form() {
        let xlsx = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../penkaku-template/tests/fixtures/merged.xlsx")).unwrap();
        let p = tmp("f.xlsx", &xlsx);
        let notes = text_of(read_import(&p, false).unwrap());
        assert!(!notes.is_empty() && !notes.contains('\t'));
        assert!(matches!(read_import(&p, true).unwrap(), ImportSource::Workbook(s) if !s.is_empty()));
    }
}
