//! 图片文档: after a document with pictures is opened or imported, offer the picture sheet (PDF,
//! A4) — every picture at the size of the space the pen leaves for it, to print, cut out and glue
//! in. 文件 › 导出 offers it again any time.

use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::*;
use penkaku_output::picture_sheet::{SheetItem, SheetOptions, picture_pdf};

use crate::doc_view::DocView;
use crate::file_ops;
use crate::state::AppState;

/// When the document has pictures: say their spaces are kept and offer the picture sheet.
/// `source`: the file they came from (the sheet is suggested beside it).
pub fn offer_sheet(view: Entity<DocView>, source: Option<PathBuf>, window: &mut Window, cx: &mut App) {
    let n = penkaku_doc::picture_count(view.read(cx).doc().document());
    if n == 0 {
        return;
    }
    window.open_dialog(cx, move |d, _w, _cx| {
        let (v, src) = (view.clone(), source.clone());
        d.title(format!("检测到 {n} 张图片"))
            .w(px(420.))
            .child(div().text_sm().child("已在报告中按图片尺寸预留位置，写字机会在每个空位的左上角写上「图k」。\n\n可以生成图片文档（PDF，A4）：图片按预留位置的大小紧凑排好，带编号和裁剪线，打印后剪下贴进对应的空位。"))
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("pic-later").small().label("以后再说").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx)))
                    .child(Button::new("pic-make").small().primary().label("生成图片文档…").on_click(move |_: &ClickEvent, w, cx| {
                        w.close_dialog(cx);
                        export_sheet(v.clone(), src.as_deref(), w, cx);
                    })),
            )
    });
}

/// Ask where to save the picture sheet (`<name>_图片.pdf` beside `source`) and write it.
pub fn export_sheet(view: Entity<DocView>, source: Option<&Path>, window: &mut Window, cx: &mut App) {
    let v = view.read(cx);
    // Each laid-out space, at its final size, with its image.
    let doc = v.doc().document();
    let items: Vec<SheetItem> = v.layout_doc().pictures.iter().map(|s| SheetItem { number: s.number, w: s.w, h: s.h, data: penkaku_doc::picture_data(doc, s.char_ix) }).collect();
    if items.is_empty() {
        return window.push_notification(Notification::info("文档里没有图片"), cx);
    }
    let font = cx.global::<AppState>().font_set.clone();
    let bytes = picture_pdf(&items, &*font, &SheetOptions::default());
    let dir = source.and_then(Path::parent).map_or_else(file_ops::documents_dir, Path::to_path_buf);
    let stem = source.and_then(Path::file_stem).map_or_else(|| "文档".to_string(), |s| s.to_string_lossy().into_owned());
    let rx = cx.prompt_for_new_path(&dir, Some(&format!("{stem}_图片.pdf")));
    let n = items.len();
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(mut path))) = rx.await else { return };
            if path.extension().is_none() {
                path.set_extension("pdf");
            }
            let msg = match std::fs::write(&path, &bytes) {
                Ok(()) => Notification::success(format!("已生成图片文档（{n} 张）：{}", path.display())),
                Err(e) => Notification::error(format!("生成失败：{e}")),
            };
            let _ = cx.update(|window, cx| window.push_notification(msg, cx));
        })
        .detach();
}
