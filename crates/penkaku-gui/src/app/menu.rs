//! The page's right-click menu commands that live outside the editor (writing, 巡边, 断点续写,
//! table tools). The menu dispatches these actions; the app root runs them.

use penkaku_device::job::PatrolMode;
use gpui_kit::*;

use super::{PenKakuApp, PageTool};

actions!(chixie_page_menu, [WritePage, WritePageSd, WriteFromHere, PatrolPage, DrawFrame, ResumeHere, OpenTune, RefreshPage, RefreshAll, BatchOverflow]);

impl PenKakuApp {
    pub(super) fn menu_actions(&self, el: Div, cx: &Context<Self>) -> Div {
        el.on_action(cx.listener(|t, _: &WritePage, w, cx| t.start_job(w, cx)))
            .on_action(cx.listener(|t, _: &WritePageSd, w, cx| t.open_sd_upload(w, cx)))
            .on_action(cx.listener(|t, _: &PatrolPage, w, cx| t.start_tool(PageTool::Patrol(PatrolMode::Up, false), w, cx)))
            .on_action(cx.listener(|t, _: &DrawFrame, w, cx| t.start_tool(PageTool::Ruling, w, cx)))
            .on_action(cx.listener(|t, _: &WriteFromHere, w, cx| t.write_from_here(w, cx)))
            .on_action(cx.listener(|t, _: &ResumeHere, _w, cx| t.resume_here(cx)))
            .on_action(cx.listener(|t, _: &OpenTune, _w, cx| t.open_tune(cx)))
            .on_action(cx.listener(|t, _: &RefreshPage, w, cx| crate::table_ui::refresh_data(&t.doc, false, w, cx)))
            .on_action(cx.listener(|t, _: &RefreshAll, w, cx| crate::table_ui::refresh_data(&t.doc, true, w, cx)))
            .on_action(cx.listener(|t, _: &BatchOverflow, w, cx| crate::table_ui::open_batch_overflow(t.doc.clone(), w, cx)))
    }
}
