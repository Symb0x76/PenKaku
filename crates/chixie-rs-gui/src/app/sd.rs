//! 上传SD卡书写: send the current page to the wireless board's SD card (`/chixie_task/`) and run
//! it there, so a long page needs no streaming link. Only 炽写 ESP32 firmware has the web API.

use chixie_rs_core::profile::Firmware;
use chixie_rs_device::esp3d::Board;
use chixie_rs_output as gcode;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::ChixieRsApp;
use crate::state::AppState;

impl ChixieRsApp {
    pub(crate) fn open_sd_upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = cx.global::<AppState>().settings.clone();
        let profile = settings.device.profile.clone();
        if profile.firmware != Firmware::ChiXieEsp32Grbl {
            window.push_notification(Notification::warning("上传SD卡书写仅支持炽写 ESP32 固件（参数设置 › 固件）"), cx);
            return;
        }
        let here = self.doc.read(cx).caret_page();
        let written = self.written_pages(cx);
        let Some(w) = written.get(here.min(written.len().saturating_sub(1))) else { return };
        if w.page.strokes.is_empty() {
            window.push_notification(Notification::warning("该页没有书写内容"), cx);
            return;
        }
        // The SD program can't stop for a pen change.
        if w.pens.iter().map(|p| p.1).collect::<std::collections::HashSet<_>>().len() > 1 {
            window.push_notification(Notification::warning("多色页面需要换笔，请直接连接书写"), cx);
            return;
        }
        let program = gcode::standalone(&gcode::emit_written(w, &profile).text, &profile);
        let name = format!("p{}.gcode", here + 1);
        let ip = cx.new(|cx| InputState::new(window, cx).placeholder("192.168.4.1").default_value(settings.device.last_ip.clone().unwrap_or_default()));
        let page_no = here + 1;
        window.open_dialog(cx, move |d, _w, cx| {
            let muted = cx.theme().muted_foreground;
            let (ip, program, name) = (ip.clone(), program.clone(), name.clone());
            d.title("上传SD卡书写")
                .w(px(380.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(h_flex().gap_2().child(div().text_sm().child("控制板 IP")).child(div().flex_1().child(Input::new(&ip).small())))
                        .child(div().text_sm().child(format!("是否立即开始书写页面：{page_no}")))
                        .child(div().text_xs().text_color(muted).child(format!("文件保存为 {}/{name}，请先确认笔尖在书写原点。", chixie_rs_device::esp3d::TASK_DIR))),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(Button::new("sd-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx)))
                        .child(Button::new("sd-ok").primary().label("上传并书写").on_click(move |_: &ClickEvent, w, cx| {
                            let addr = ip.read(cx).value().trim().to_string();
                            if addr.is_empty() {
                                w.push_notification(Notification::warning("请输入控制板 IP"), cx);
                                return;
                            }
                            let st = cx.global_mut::<AppState>();
                            st.settings.device.last_ip = Some(addr.clone());
                            st.save_settings();
                            w.close_dialog(cx);
                            let (program, name) = (program.clone(), name.clone());
                            let task = cx.background_executor().spawn(async move { Board::new(&addr).and_then(|b| b.upload_and_run(&name, &program)) });
                            w.spawn(cx, async move |cx| {
                                let msg = match task.await {
                                    Ok(()) => Notification::success("已上传，控制板开始书写"),
                                    Err(e) => Notification::error(format!("上传SD卡失败：{e}")),
                                };
                                let _ = cx.update(|w, cx| w.push_notification(msg, cx));
                            })
                            .detach();
                        })),
                )
        });
    }
}
