//! 固件参数: the board's Grbl `$` settings as an editable table (参数 / 值 / 说明). 应用 sends only
//! the changed rows, each acknowledged, then reads `$$` back; 恢复默认 sends `$RST=*` after the
//! user types 恢复. Everything is disabled while a job runs (the device actor is busy then).

use penkaku_device::device::Command;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::device::DevicePanel;

/// Plain-language meaning of a setting (classic Grbl 1.1 numbers, and the Grbl_ESP32 named keys
/// the docs list). Written for this app; unknown keys show nothing.
pub fn describe(key: &str) -> &'static str {
    match key {
        "0" => "步进脉冲宽度（µs）",
        "1" => "步进空闲延时（ms，255 = 保持锁定）",
        "2" => "步进脉冲反向（位掩码 XYZ）",
        "3" => "运动方向反向（位掩码 XYZ）",
        "4" => "步进使能信号反向",
        "5" => "限位开关信号反向",
        "6" => "探针信号反向",
        "10" => "状态报告内容（位掩码）",
        "11" => "拐角偏差（mm）",
        "12" => "圆弧精度（mm）",
        "13" => "以英寸报告",
        "20" => "软限位",
        "21" => "硬限位",
        "22" => "回原点功能",
        "23" => "回原点方向反向（位掩码 XYZ）",
        "24" => "回原点慢速（mm/min）",
        "25" => "回原点快速（mm/min）",
        "26" => "回原点去抖（ms）",
        "27" => "回原点后退距离（mm）",
        "30" => "主轴最高转速 / PWM 上限",
        "31" => "主轴最低转速 / PWM 下限",
        "32" => "激光模式",
        "100" => "X 每毫米步数",
        "101" => "Y 每毫米步数",
        "102" => "Z 每毫米步数",
        "110" => "X 最大速度（mm/min）",
        "111" => "Y 最大速度（mm/min）",
        "112" => "Z 最大速度（mm/min，抬落笔）",
        "120" => "X 加速度（mm/s²）",
        "121" => "Y 加速度（mm/s²）",
        "122" => "Z 加速度（mm/s²，抬落笔）",
        "130" => "X 最大行程（mm）",
        "131" => "Y 最大行程（mm）",
        "132" => "Z 最大行程（mm）",
        "Stepper/IdleTime" => "步进空闲时间（ms，255 = 保持锁定）",
        "ChiXie/AxisFollow" => "A 轴模式（0 独立、1 跟随X、2 跟随Y、3 跟随Z）",
        "ChiXie/MachineMode" => "传动类型（0 XY、1 CoreXY）",
        "ChiXie/ForceSetOrign" => "回原点后强制设为原点",
        "Spindle/PWM/Frequency" => "主轴 PWM 频率（Hz）",
        "Radio/Mode" => "无线模式（None / STA / AP / BT）",
        _ => named(key),
    }
}

/// Grbl_ESP32 per-axis keys (`X/StepsPerMm`, …).
fn named(key: &str) -> &'static str {
    match key.split_once('/').map(|(_, rest)| rest) {
        Some("StepsPerMm") => "每毫米步数",
        Some("MaxRate") => "最大速度（mm/min）",
        Some("Acceleration") => "加速度（mm/s²）",
        Some("MaxTravel") => "最大行程（mm）",
        Some("ChiXie/HomeOrder") => "回原点顺序",
        _ => "",
    }
}

struct Row {
    key: String,
    value: String,
    input: Entity<InputState>,
}

pub struct FirmwareView {
    device: Entity<DevicePanel>,
    rows: Vec<Row>,
    _subs: Vec<Subscription>,
}

impl FirmwareView {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn new(device: Entity<DevicePanel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let params = device.read(cx).params.clone();
        let sub = cx.observe_in(&params, window, |this, _p, window, cx| this.load(window, cx));
        let mut this = Self { device, rows: Vec::new(), _subs: vec![sub] };
        this.load(window, cx);
        this
    }

    /// Rebuild the rows from the last `$$` reply (after opening, 应用 or 恢复默认).
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let machine = self.device.read(cx).params.read(cx).machine.clone();
        let all = machine.map(|m| m.all).unwrap_or_default();
        if all.len() == self.rows.len() && all.iter().zip(&self.rows).all(|((k, v), r)| *k == r.key && *v == r.value) {
            return;
        }
        self.rows = all
            .into_iter()
            .map(|(key, value)| {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(value.clone()));
                Row { key, value, input }
            })
            .collect();
        cx.notify();
    }

    fn changed(&self, cx: &App) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter_map(|r| {
                let v = r.input.read(cx).value().trim().to_string();
                (!v.is_empty() && v != r.value).then(|| (r.key.clone(), v))
            })
            .collect()
    }

    fn usable(&self, cx: &App) -> bool {
        let d = self.device.read(cx);
        d.is_connected() && !d.busy()
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changed = self.changed(cx);
        if changed.is_empty() {
            window.push_notification(Notification::info("没有改动"), cx);
            return;
        }
        let n = changed.len();
        self.device.read(cx).send(Command::WriteSettings(changed));
        window.push_notification(Notification::info(format!("正在写入 {n} 项参数…")), cx);
    }

    fn confirm_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let device = self.device.clone();
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder("恢复"));
        window.open_dialog(cx, move |d, _w, _cx| {
            let (device, typed) = (device.clone(), typed.clone());
            d.title("恢复默认值").w(px(380.)).child(
                v_flex()
                    .gap_2()
                    .child("将把控制板的全部 Grbl 参数恢复为固件默认值（$RST=*），当前设置（行程、速度、步数等）都会丢失。")
                    .child("确认请输入「恢复」：")
                    .child(Input::new(&typed).small()),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(Button::new("rst-cancel").label("取消").on_click(|_: &ClickEvent, w, cx| w.close_dialog(cx)))
                    .child(Button::new("rst-ok").danger().label("恢复默认").on_click(move |_: &ClickEvent, w, cx| {
                        if typed.read(cx).value().trim() != "恢复" {
                            w.push_notification(Notification::warning("请输入「恢复」以确认"), cx);
                            return;
                        }
                        device.read(cx).send(Command::ResetSettings);
                        w.close_dialog(cx);
                    })),
            )
        });
    }
}

impl Render for FirmwareView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, accent, border) = (theme.muted_foreground, theme.primary, theme.border);
        let usable = self.usable(cx);
        let changed = self.changed(cx).len();
        let mut list = v_flex().id("fw-rows").gap_1().max_h(px(420.)).overflow_y_scroll();
        if self.rows.is_empty() {
            list = list.child(div().text_sm().text_color(muted).child("尚未读取到固件参数。请连接设备后点「重新读取」。"));
        }
        for (i, r) in self.rows.iter().enumerate() {
            let edited = r.input.read(cx).value().trim() != r.value;
            list = list.child(
                h_flex()
                    .id(("fw-row", i))
                    .gap_2()
                    .py_0p5()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(150.)).text_sm().font_family("monospace").text_color(if edited { accent } else { theme.foreground }).child(format!("${}", r.key)))
                    .child(div().w(px(120.)).child(Input::new(&r.input).small().disabled(!usable)))
                    .child(div().flex_1().text_xs().text_color(muted).child(describe(&r.key))),
            );
        }
        v_flex()
            .gap_2()
            .child(list)
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("fw-read").small().label("重新读取").disabled(!usable).on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.device.read(cx).send(Command::QueryMachine))))
                    .child(Button::new("fw-reset").small().danger().label("恢复默认").disabled(!usable).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.confirm_reset(w, cx))))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child(if usable { format!("{changed} 项改动") } else { "设备未连接或正在书写".to_string() }))
                    .child(Button::new("fw-apply").small().primary().label("应用").disabled(!usable || changed == 0).on_click(cx.listener(|t, _: &ClickEvent, w, cx| t.apply(w, cx)))),
            )
    }
}

/// 设备 › 固件参数…
pub fn open(device: Entity<DevicePanel>, window: &mut Window, cx: &mut App) {
    if device.read(cx).params.read(cx).machine.is_none() {
        device.read(cx).send(Command::QueryMachine);
    }
    let view = cx.new(|cx| FirmwareView::new(device, window, cx));
    window.open_dialog(cx, move |d, _w, _cx| d.title("固件参数").w(px(640.)).child(view.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn describes_classic_and_named_keys() {
        assert_eq!(describe("110"), "X 最大速度（mm/min）");
        assert_eq!(describe("Y/StepsPerMm"), "每毫米步数");
        assert_eq!(describe("Sta/Password"), "");
    }
}
