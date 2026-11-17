//! 断点续写: continue a page from a chosen character. The user selects the first character that
//! was not written yet (or keeps the one remembered from the last interrupted job) and presses
//! 开始续写; the page is re-emitted from that character's first stroke, byte-identical to the
//! uninterrupted tail, then the following pages run as usual.

use chixie_rs_output::{self as gcode, PageCode};
use chixie_rs_core::layout::Page;
use chixie_rs_device::device::{Event as DevEvent, Job, JobOutcome, PLANNER_DEPTH};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChixieRsApp;
use crate::state::AppState;

/// What a running job's lines mean, to name the character where it stopped.
pub(crate) struct JobMeta {
    first_page: usize,
    /// Per job page: (line of each stroke's `G0`, char index of each stroke).
    pages: Vec<(Vec<usize>, Vec<usize>)>,
    /// Character ids at job start, so the break point survives later edits.
    ids: Vec<u64>,
}

impl JobMeta {
    pub fn new(first_page: usize, codes: &[PageCode], pages: &[Page], ids: &[u64]) -> Self {
        let pages = codes.iter().zip(pages).map(|(c, p)| (c.stroke_lines.clone(), p.stroke_chars.clone())).collect();
        Self { first_page, pages, ids: ids.to_vec() }
    }

    /// Id of the character drawn by the stroke that holds line `line` of document page `page`.
    fn char_id(&self, page: usize, line: usize) -> Option<u64> {
        let (lines, chars) = self.pages.get(page.checked_sub(self.first_page)?)?;
        let k = lines.partition_point(|&s| s <= line).saturating_sub(1);
        self.ids.get(*chars.get(k)?).copied()
    }
}

#[derive(Default)]
pub(crate) struct Resume {
    /// The 断点续写 card is shown in the task pane.
    pub open: bool,
    meta: Option<JobMeta>,
    /// Character the last interrupted job would continue from.
    break_id: Option<u64>,
}

/// Why 开始续写 can't run, or the character index it would start from.
enum Pick {
    None,
    Bad(&'static str),
    At(usize),
}

impl Resume {
    pub fn job_started(&mut self, meta: Option<JobMeta>) {
        self.meta = meta;
    }

    /// Watch device events for the break point. Returns the character the job will continue
    /// from after a reconnect (for the toast).
    pub fn observe(&mut self, ev: &DevEvent, chars: &[char], ids: &[u64]) -> Option<char> {
        let meta = self.meta.as_ref()?;
        let id = match ev {
            // `line` is the pen-up before the stroke the actor resumes with.
            DevEvent::Recovered { page, line, .. } => meta.char_id(*page, line + 1),
            // Up to a planner's worth of acked lines may not have been drawn.
            DevEvent::JobEnded { outcome: JobOutcome::Stopped | JobOutcome::Failed(_), page, line } => {
                meta.char_id(*page, line.saturating_sub(PLANNER_DEPTH))
            }
            DevEvent::JobEnded { outcome: JobOutcome::Done, .. } => {
                (self.meta, self.break_id) = (None, None);
                return None;
            }
            _ => return None,
        }?;
        self.break_id = Some(id);
        ids.iter().position(|&i| i == id).map(|i| chars[i])
    }
}

impl ChixieRsApp {
    pub(crate) fn open_resume(&mut self, cx: &mut Context<Self>) {
        (self.resume.open, self.show_pane) = (true, true);
        cx.notify();
    }

    fn resume_pick(&self, cx: &App) -> Pick {
        let doc = self.doc.read(cx).doc();
        let sel = doc.selection();
        let i = match sel.len() {
            0 => match self.resume.break_id.and_then(|id| doc.ids().iter().position(|&x| x == id)) {
                Some(i) => i,
                None => return Pick::None,
            },
            1 => sel.start,
            _ => return Pick::Bad("请选中待续写区文本中第一个字符，不要选中多个"),
        };
        if doc.chars()[i].is_whitespace() {
            return Pick::Bad("请选中有内容的字符，不要选中空格");
        }
        Pick::At(i)
    }

    pub(crate) fn start_resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let device = self.device.read(cx);
        if device.connected.is_none() || device.busy() {
            window.push_notification(Notification::warning("请先连接串口或先等待无书写任务时再续写"), cx);
            return;
        }
        let Pick::At(i) = self.resume_pick(cx) else { return };
        let layout = self.doc.read(cx).document();
        let Some(page_ix) = layout.boxes.get(i).map(|b| b.page) else { return };
        let profile = cx.global::<AppState>().settings.device.profile.clone();
        let written = self.written_pages(cx);
        let pages = &written[page_ix..];
        let mut codes: Vec<PageCode> = pages.iter().map(|p| gcode::emit_mapped(p, &profile)).collect();
        let k = pages[0].first_stroke_from(i);
        codes[0] = codes[0].from_stroke(k);
        let mut first = pages[0].clone();
        first.stroke_chars.drain(..k.min(first.stroke_chars.len()));
        let metas: Vec<Page> = std::iter::once(first).chain(pages[1..].iter().cloned()).collect();
        let ids = self.doc.read(cx).doc().ids().to_vec();
        self.resume.job_started(Some(JobMeta::new(page_ix, &codes, &metas, &ids)));
        let job = Job { pages: codes.iter().map(|c| c.text.clone()).collect(), first_page: page_ix, maps: codes.into_iter().map(|c| c.stroke_lines).collect() };
        self.resume.open = false;
        self.device.update(cx, |d, cx| d.run(job, cx));
    }

    /// The 断点续写 card at the top of the task pane.
    pub(crate) fn resume_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, warn) = (theme.muted_foreground, theme.warning);
        let pick = self.resume_pick(cx);
        let doc = self.doc.read(cx).doc();
        let (label, problem) = match pick {
            Pick::None => ("未选中".to_string(), None),
            Pick::Bad(m) => ("未选中".to_string(), Some(m)),
            Pick::At(i) => (format!("「{}」", doc.chars()[i]), None),
        };
        let ready = matches!(pick, Pick::At(_));
        v_flex()
            .gap_1()
            .p_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("断点续写"))
                    .child(Button::new("resume-close").ghost().xsmall().label("关闭").on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.resume.open = false;
                        cx.notify();
                    }))),
            )
            .child(h_flex().gap_1().text_sm().child("续写起始字符：").child(div().font_weight(FontWeight::SEMIBOLD).child(label)))
            .child(div().text_xs().text_color(muted).child("提示：请选中续写的起始字符，然后点击开始"))
            .when_some(problem, |el, m| el.child(div().text_xs().text_color(warn).child(m)))
            .child(div().text_xs().text_color(warn).child("注意：请保证已经设置过原点（纸张移动过就先点动到原点并「设为原点」）"))
            .child(
                Button::new("resume-start")
                    .small()
                    .primary()
                    .label("开始续写")
                    .disabled(!ready)
                    .on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.start_resume(w, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn break_point_names_the_character() {
        // Page 2 of a job starting at page 2: strokes at lines 1, 6, 11 draw chars 0, 0, 3.
        let codes = [PageCode { text: String::new(), stroke_lines: vec![1, 6, 11] }];
        let pages = [Page { stroke_chars: vec![0, 0, 3], ..Default::default() }];
        let meta = JobMeta::new(2, &codes, &pages, &[10, 11, 12, 13]);
        assert_eq!(meta.char_id(2, 7), Some(10));
        assert_eq!(meta.char_id(2, 12), Some(13));
        assert_eq!(meta.char_id(2, 0), Some(10), "header line: first stroke");
        assert_eq!(meta.char_id(1, 7), None, "page outside the job");

        let mut r = Resume::default();
        r.job_started(Some(meta));
        let chars = ['a', 'b', 'c', 'd'];
        let ev = DevEvent::Recovered { page: 2, line: 10, rebooted: false };
        assert_eq!(r.observe(&ev, &chars, &[10, 11, 12, 13]), Some('d'));
        assert_eq!(r.break_id, Some(13));
        r.observe(&DevEvent::JobEnded { outcome: JobOutcome::Done, page: 2, line: 0 }, &chars, &[]);
        assert_eq!(r.break_id, None);
    }
}
