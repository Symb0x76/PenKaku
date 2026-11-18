//! 导入 PDF/图片 into a table form: detect the frames locally (`chixie-rs-import`), confirm the
//! scale and place them, keep the scan as the page background, and 内边距识别 from it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chixie_rs_import::{DetectedPage, DetectedTable};
use chixie_rs_template::TemplateKind;
use chixie_rs_template::table::{Background, Cell, Span, Table, TablePage};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputState, NumberInput};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use image::{Frame, GrayImage, RgbaImage};

use super::TemplatesView;
use crate::state::AppState;

/// A loaded page background: which file at which opacity, its pixels, and the gpui image.
pub struct BgImage {
    pub key: (String, u8),
    pub grey: GrayImage,
    pub render: Arc<RenderImage>,
}

/// Where page backgrounds live: `template_assets/` next to `templates.toml`.
fn assets_dir(cx: &App) -> PathBuf {
    let p = &cx.global::<AppState>().templates_path;
    p.parent().unwrap_or(Path::new(".")).join("template_assets")
}

fn render_grey(grey: &GrayImage, opacity: f32) -> Arc<RenderImage> {
    let a = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    // gpui wants BGRA, straight alpha.
    let buf: Vec<u8> = grey.as_raw().iter().flat_map(|&g| [g, g, g, a]).collect();
    let rgba = RgbaImage::from_raw(grey.width(), grey.height(), buf).expect("buffer matches size");
    Arc::new(RenderImage::new(vec![Frame::new(rgba)]))
}

/// A detected table in page units → a template table at mm `(ox, oy)` for its corner, `k` mm
/// per unit.
fn to_table(t: &DetectedTable, k: f32, ox: f32, oy: f32) -> Table {
    let cells = t.cells.iter().map(|&(r0, c0, r1, c1)| Cell { span: Span { r0, c0, r1, c1 }, ..Default::default() }).collect();
    Table { x: ox, y: oy, locked: false, cols: t.cols.iter().map(|c| c * k).collect(), rows: t.rows.iter().map(|r| r * k).collect(), cells }
}

impl TemplatesView {
    /// The current page's background for painting (file loaded once per file / opacity).
    pub(super) fn background(&self, cx: &App) -> Option<(Background, Arc<RenderImage>)> {
        if self.tbl.hide_bg {
            return None;
        }
        let TemplateKind::Table(tt) = &self.draft.as_ref()?.kind else { return None };
        let bg = tt.pages.get(self.tbl.page)?.background.clone()?;
        let key = (bg.file.clone(), (bg.opacity * 20.0).round() as u8);
        let mut cache = self.tbl.bg_cache.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.key.0 != key.0) {
            let grey = image::open(assets_dir(cx).join(&bg.file)).ok()?.to_luma8();
            let render = render_grey(&grey, bg.opacity);
            *cache = Some(BgImage { key: key.clone(), grey, render });
        } else if let Some(c) = cache.as_mut()
            && c.key != key
        {
            c.render = render_grey(&c.grey, bg.opacity);
            c.key = key;
        }
        cache.as_ref().map(|c| (bg, c.render.clone()))
    }

    /// 导入 PDF/图片…: pick a file and detect it.
    pub(super) fn import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("PDF或图片(*.pdf *.jpg *.png)".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            if let Some(p) = paths.into_iter().next() {
                let _ = this.update_in(cx, |v, w, cx| v.import_path(p, w, cx));
            }
        })
        .detach();
    }

    /// Detect `path` off the UI thread, then show the result.
    pub(super) fn import_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        window.push_notification(Notification::info("正在识别表格…"), cx);
        let task = cx.background_executor().spawn(async move { std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| chixie_rs_import::detect(&b)) });
        cx.spawn_in(window, async move |this, cx| {
            let res = task.await;
            let _ = this.update_in(cx, |v, w, cx| match res {
                Ok(pages) if pages.iter().any(|p| !p.tables.is_empty()) => v.import_result(pages, w, cx),
                Ok(pages) => w.push_notification(Notification::warning(chixie_rs_import::failed_pages_message(&pages).unwrap_or_else(|| "未识别到表格".into())), cx),
                Err(e) => w.push_notification(Notification::error(format!("导入失败：{e}")), cx),
            });
        })
        .detach();
    }

    /// Confirm the scale and place, then apply (fixes afterwards in the editor).
    fn import_result(&mut self, pages: Vec<DetectedPage>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(first) = pages.iter().find_map(|p| p.tables.first().map(|t| (t.clone(), p.mm_per_unit))) else { return };
        let (t0, k) = first;
        let paper = self.draft.as_ref().map_or((210.0, 297.0), |t| t.paper);
        // Known scale: keep the detected size and place; unknown: fit the page width with 20 mm margins.
        let (w0, x0, y0) = match k {
            Some(k) => (t0.width() * k, t0.x * k, t0.y * k),
            None => ((paper.0 - 40.0).min(t0.width()), 20.0, 20.0),
        };
        let input = |v: f32, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).default_value(format!("{v:.1}")));
        let (width, x, y) = (input(w0, window, cx), input(x0, window, cx), input(y0, window, cx));
        let view = cx.entity();
        let pages = Arc::new(pages);
        let scale_known = k.is_some();
        window.open_dialog(cx, move |d, _w, cx| {
            let theme = cx.theme();
            let (muted, accent) = (theme.muted_foreground, theme.primary);
            let (pages, view, width, x, y) = (pages.clone(), view.clone(), width.clone(), x.clone(), y.clone());
            let preview_pages = pages.clone();
            // The first page's image (or a blank sheet) with the detected grid on top.
            let preview = canvas(
                |_, _, _| (),
                move |bounds, (), window, _cx| {
                    let p = &preview_pages[0];
                    let (iw, ih) = p.background.as_ref().map_or_else(|| p.size_mm.unwrap_or((210.0, 297.0)), |b| (b.width() as f32, b.height() as f32));
                    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    let z = (bw / iw).min(bh / ih);
                    let o = bounds.origin + point(px((bw - iw * z) / 2.0), px((bh - ih * z) / 2.0));
                    let area = Bounds::new(o, size(px(iw * z), px(ih * z)));
                    match &p.background {
                        Some(b) => {
                            let _ = window.paint_image(area, area, Corners::default(), render_grey(b, 1.0), 0, false);
                        }
                        None => window.paint_quad(fill(area, gpui_kit::white())),
                    }
                    for t in &p.tables {
                        for &(r0, c0, r1, c1) in &t.cells {
                            let x0 = t.x + t.cols[..c0 as usize].iter().sum::<f32>();
                            let y0 = t.y + t.rows[..r0 as usize].iter().sum::<f32>();
                            let w: f32 = t.cols[c0 as usize..c1 as usize].iter().sum();
                            let h: f32 = t.rows[r0 as usize..r1 as usize].iter().sum();
                            let r = Bounds::new(o + point(px(x0 * z), px(y0 * z)), size(px(w * z), px(h * z)));
                            window.paint_quad(outline(r, accent, BorderStyle::Solid));
                        }
                    }
                },
            )
            .size_full();
            let count: usize = pages.iter().map(|p| p.tables.iter().map(|t| t.cells.len()).sum::<usize>()).sum();
            d.title("导入表格")
                .w(px(560.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().h(px(300.)).rounded_md().bg(theme.muted).p_2().child(preview))
                        .child(div().text_sm().child(format!("识别到 {} 页、{count} 个单元格（蓝框）。确认后可在编辑器里合并、拆分或拖动表格线修正。", pages.len())))
                        .child(h_flex().gap_2().child(div().w(px(84.)).text_sm().text_color(muted).child("表格宽度：")).child(div().w(px(120.)).child(NumberInput::new(&width).small().suffix("mm"))).when(scale_known, |el| {
                            el.child(div().text_xs().text_color(muted).child("（按文件中的尺寸）"))
                        }))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().w(px(84.)).text_sm().text_color(muted).child("横向坐标："))
                                .child(div().w(px(120.)).child(NumberInput::new(&x).small().suffix("mm")))
                                .child(div().text_sm().text_color(muted).child("纵向坐标："))
                                .child(div().w(px(120.)).child(NumberInput::new(&y).small().suffix("mm"))),
                        ),
                )
                .footer(h_flex().justify_end().gap_2().child(Button::new("im-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx))).child(
                    Button::new("im-ok").primary().label("确认").on_click(move |_: &ClickEvent, w, cx| {
                        let n = |e: &Entity<InputState>| e.read(cx).value().trim().parse::<f32>().ok().filter(|v| v.is_finite());
                        let (wd, xx, yy) = (n(&width).unwrap_or(w0).max(5.0), n(&x).unwrap_or(x0).max(0.0), n(&y).unwrap_or(y0).max(0.0));
                        let pages = pages.clone();
                        view.update(cx, |v, cx| v.apply_import(&pages, wd, xx, yy, w, cx));
                        w.close_dialog(cx);
                    }),
                ))
        });
    }

    /// Put the detected pages into the form from the current page on: the first table's width
    /// becomes `width` mm with its corner at (`x`, `y`); everything else keeps its proportions.
    pub(crate) fn apply_import(&mut self, pages: &[DetectedPage], width: f32, x: f32, y: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t0) = pages.iter().find_map(|p| p.tables.first().cloned()) else { return };
        let k = width / t0.width().max(1e-3);
        let dir = assets_dir(cx);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let mut new_pages: Vec<TablePage> = Vec::new();
        for (i, p) in pages.iter().enumerate() {
            let tables = p.tables.iter().map(|t| to_table(t, k, x + (t.x - t0.x) * k, y + (t.y - t0.y) * k)).collect();
            let background = p.background.as_ref().and_then(|img| {
                let file = format!("bg-{stamp}-{}.png", i + 1);
                std::fs::create_dir_all(&dir).ok()?;
                img.save(dir.join(&file)).ok()?;
                let (w, h) = (img.width() as f32 * k, img.height() as f32 * k);
                Some(Background { file, x: x - t0.x * k, y: y - t0.y * k, w, h, opacity: 0.5 })
            });
            new_pages.push(TablePage { tables, background, ..Default::default() });
        }
        // A landscape source on a portrait page (or the other way) turns the paper.
        let landscape = pages.first().is_some_and(|p| p.landscape);
        if let Some(t) = &mut self.draft
            && (t.paper.0 > t.paper.1) != landscape
        {
            t.paper = (t.paper.1, t.paper.0);
            window.push_notification(Notification::info("检测到导入的模板与页面方向不同，已为您切换"), cx);
        }
        let start = self.tbl.page;
        self.edit_tt(cx, |tt| {
            for (i, p) in new_pages.into_iter().enumerate() {
                match tt.pages.get_mut(start + i) {
                    Some(slot) => *slot = TablePage { excel_range: slot.excel_range, ..p },
                    None => tt.pages.push(p),
                }
            }
        });
        if let Some(msg) = chixie_rs_import::failed_pages_message(pages) {
            window.push_notification(Notification::warning(msg), cx);
        }
        self.tbl.sel.clear();
        *self.tbl.bg_cache.borrow_mut() = None;
    }

    /// Autotest: move on to a new blank page (the next import goes there).
    pub(crate) fn add_page_for_test(&mut self, cx: &mut Context<Self>) {
        self.edit_tt(cx, |tt| tt.add_page());
        self.tbl.page = self.draft_tables().0.len().saturating_sub(1);
    }

    /// Autotest: the cell count of each table on each page of the draft, and the background files.
    pub(crate) fn draft_tables(&self) -> (Vec<Vec<usize>>, Vec<String>) {
        let Some(TemplateKind::Table(tt)) = self.draft.as_ref().map(|t| &t.kind) else { return Default::default() };
        let cells = tt.pages.iter().map(|p| p.tables.iter().map(|t| t.cells.len()).collect()).collect();
        let files = tt.pages.iter().filter_map(|p| p.background.as_ref().map(|b| b.file.clone())).collect();
        (cells, files)
    }

    /// 内边距识别: each selected cell's left padding = where its first ink is in the background.
    pub(super) fn detect_padding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tbl.sel.is_empty() {
            window.push_notification(Notification::warning("请先选中表格单元格再识别"), cx);
            return;
        }
        let page = self.tbl.page;
        let Some(TemplateKind::Table(tt)) = self.draft.as_ref().map(|t| &t.kind) else { return };
        let Some(bg) = tt.pages[page].background.clone() else {
            window.push_notification(Notification::warning("所在页面没有背景图，无法识别"), cx);
            return;
        };
        let Ok(img) = image::open(assets_dir(cx).join(&bg.file)).map(|i| i.to_luma8()) else {
            window.push_notification(Notification::warning("所在页面没有背景图，无法识别"), cx);
            return;
        };
        let (sx, sy) = (img.width() as f32 / bg.w, img.height() as f32 / bg.h);
        let mut found = Vec::new();
        for &(t, c) in &self.tbl.sel {
            let table = &tt.pages[page].tables[t];
            let (x0, y0, w, h) = table.rect(table.cells[c].span);
            let rect = (((x0 - bg.x) * sx).max(0.0) as u32, ((y0 - bg.y) * sy).max(0.0) as u32, ((x0 + w - bg.x) * sx).max(0.0) as u32, ((y0 + h - bg.y) * sy).max(0.0) as u32);
            // Skip the cell's own border lines: about 1 mm.
            if let Some(dx) = chixie_rs_import::left_padding(&img, rect, sx.ceil() as u32) {
                found.push((t, c, dx as f32 / sx));
            }
        }
        let n = found.len();
        self.edit_tt(cx, |tt| {
            for (t, c, pad) in found {
                tt.pages[page].tables[t].cells[c].padding.0 = (pad * 10.0).round() / 10.0;
            }
        });
        window.push_notification(Notification::success(format!("已识别 {n} 个单元格的左内边距")), cx);
    }

    /// 重置内边距: back to 1 mm.
    pub(super) fn reset_padding(&mut self, cx: &mut Context<Self>) {
        let (page, sel) = (self.tbl.page, self.tbl.sel.clone());
        self.edit_tt(cx, |tt| {
            for (t, c) in sel {
                tt.pages[page].tables[t].cells[c].padding.0 = 1.0;
            }
        });
    }

    /// 透明度: change the page background's opacity by `d`.
    pub(super) fn bump_opacity(&mut self, d: f32, cx: &mut Context<Self>) {
        let page = self.tbl.page;
        self.edit_tt(cx, |tt| {
            if let Some(b) = &mut tt.pages[page].background {
                b.opacity = (b.opacity + d).clamp(0.1, 1.0);
            }
        });
    }
}
