//! 导出 PDF and 打印: the pages as vector PDF at real size (打印文字 / 打印背景图). Printing writes
//! a temporary PDF and hands it to the system (gpui has no print API): Windows prints it with the
//! PDF viewer's Print verb (or opens it when none is registered), macOS opens it in Preview,
//! Linux in the default viewer.

use std::path::{Path, PathBuf};

use chixie_rs_output::pdf::{PdfOptions, pdf};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::file_ops;
use crate::state::AppState;

impl ChixieRsApp {
    /// The document as PDF bytes, or a message when there is nothing to print.
    fn pdf_bytes(&self, cx: &App) -> Result<Vec<u8>, &'static str> {
        let st = cx.global::<AppState>();
        let (page, ui) = (&st.settings.page, &st.settings.ui);
        if !ui.print_text && !ui.print_background {
            return Err("请至少勾选「打印文字」或「打印背景图」");
        }
        let doc = self.doc.read(cx).document();
        let has_guides = doc.pages.iter().any(|p| !p.guides.is_empty());
        if !(ui.print_text && doc.stroke_count() > 0 || ui.print_background && has_guides) {
            return Err("没有可打印的内容");
        }
        let d = self.doc.read(cx).doc();
        let colours = (0..d.len()).map(|i| d.document().attrs(i).colour).collect();
        let o = PdfOptions { ink: ui.print_text, background: ui.print_background, paper: page.paper, margins: page.margins, colours, ..Default::default() };
        Ok(pdf(&doc, &o))
    }

    pub(super) fn export_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bytes = match self.pdf_bytes(cx) {
            Ok(b) => b,
            Err(m) => return window.push_notification(Notification::warning(m), cx),
        };
        let rx = cx.prompt_for_new_path(&file_ops::documents_dir(), Some("文档.pdf"));
        cx.spawn_in(window, async move |_this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else { return };
            if path.extension().is_none() {
                path.set_extension("pdf");
            }
            let msg = match std::fs::write(&path, &bytes) {
                Ok(()) => Notification::success(format!("已导出 {}", path.display())),
                Err(e) => Notification::error(format!("导出失败：{e}")),
            };
            let _ = cx.update(|window, cx| window.push_notification(msg, cx));
        })
        .detach();
    }

    pub(super) fn print_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bytes = match self.pdf_bytes(cx) {
            Ok(b) => b,
            Err(m) => return window.push_notification(Notification::warning(m), cx),
        };
        let path = std::env::temp_dir().join("chixie-rs-print.pdf");
        let task = cx.background_executor().spawn(async move { std::fs::write(&path, &bytes).and_then(|()| print_file(&path)) });
        cx.spawn_in(window, async move |_this, cx| {
            let msg = match task.await {
                Ok(()) => Notification::info("已交给系统打印（请在打开的窗口中确认打印机）"),
                Err(e) => Notification::error(format!("打印失败：{e}")),
            };
            let _ = cx.update(|window, cx| window.push_notification(msg, cx));
        })
        .detach();
    }
}

/// Hand a PDF to the system for printing.
fn print_file(path: &Path) -> std::io::Result<()> {
    use std::process::Command;
    let p: PathBuf = path.to_path_buf();
    let ok = |s: std::process::ExitStatus| if s.success() { Ok(()) } else { Err(std::io::Error::other(format!("exit {s}"))) };
    if cfg!(target_os = "windows") {
        let quoted = p.display().to_string().replace('\'', "''");
        let print = Command::new("powershell").args(["-NoProfile", "-Command", &format!("Start-Process -FilePath '{quoted}' -Verb Print")]).status();
        match print {
            Ok(s) if s.success() => Ok(()),
            // No viewer with a Print verb: open it, the viewer prints.
            _ => ok(Command::new("cmd").args(["/C", "start", ""]).arg(&p).status()?),
        }
    } else if cfg!(target_os = "macos") {
        ok(Command::new("open").args(["-a", "Preview"]).arg(&p).status()?)
    } else {
        ok(Command::new("xdg-open").arg(&p).status()?)
    }
}
