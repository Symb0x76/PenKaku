//! 设备 › 参数设置: the device profile (ChiXie3's 参数设置: 设备名, 固件, 设备类型, 写字速度,
//! 落笔后暂停, 落笔高度 …) plus the machine's own specs read with `$$`. Every edit is saved to the
//! settings and sent to the device actor at once.

use chixie_rs_core::chixie3_devices;
use chixie_rs_device::device::{Command, DeviceHandle, Pen};
use chixie_rs_core::profile::{DeviceProfile, DeviceType, Firmware, Origin, PageRotation, PathPlan};
use chixie_rs_device::grbl::MachineSettings;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum F {
    WriteSpeed,
    TravelSpeed,
    Pause,
    UpZ,
    DownZ,
    PressStep,
}

const NUMS: [(F, &str, &str, f64, f64); 6] = [
    (F::WriteSpeed, "写字速度", "mm/min", 100.0, 500.0),
    (F::TravelSpeed, "空走速度", "mm/min", 100.0, 500.0),
    (F::Pause, "落笔后暂停", "秒", 0.0, 0.1),
    (F::UpZ, "抬笔高度", "mm", -50.0, 0.5),
    (F::DownZ, "落笔高度", "mm", -50.0, 0.5),
    (F::PressStep, "轻重每级", "mm", 0.0, 0.05),
];

pub struct DeviceParams {
    handle: DeviceHandle,
    name: Entity<InputState>,
    nums: Vec<(F, Entity<InputState>)>,
    custom_up: Entity<InputState>,
    custom_down: Entity<InputState>,
    pub machine: Option<MachineSettings>,
    /// Mirrors the device pane's connection (enables the $$ refresh).
    pub connected: bool,
    /// The profile the form last showed / wrote; anything else in the settings is an outside
    /// change (import, another view) and reloads the form.
    shown: DeviceProfile,
    loading: bool,
    _subs: Vec<Subscription>,
}

fn fmt_num(v: f32) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl DeviceParams {
    pub fn new(handle: DeviceHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let custom_up = cx.new(|cx| InputState::new(window, cx).placeholder("如 M5"));
        let custom_down = cx.new(|cx| InputState::new(window, cx).placeholder("如 M3 S90"));
        let mut subs = Vec::new();
        for e in [&name, &custom_up, &custom_down] {
            subs.push(cx.subscribe_in(e, window, |this, _s, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.read_form(cx);
                }
            }));
        }
        let mut nums = Vec::new();
        for (f, _, _, min, step) in NUMS {
            let st = cx.new(|cx| InputState::new(window, cx).min(min).step(step));
            subs.push(cx.subscribe_in(&st, window, |this, _s, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.read_form(cx);
                }
            }));
            nums.push((f, st));
        }
        let mut this = Self { handle, name, nums, custom_up, custom_down, machine: None, connected: false, shown: DeviceProfile::default(), loading: false, _subs: subs };
        this._subs.push(cx.observe_global_in::<AppState>(window, |this, window, cx| {
            let p = &cx.global::<AppState>().settings.device.profile;
            if *p != this.shown {
                this.handle.send(Command::SetProfile(p.clone()));
                this.load(window, cx);
            }
        }));
        this.load(window, cx);
        this
    }

    fn input(&self, f: F) -> Entity<InputState> {
        self.nums.iter().find(|(k, _)| *k == f).expect("all fields").1.clone()
    }

    fn profile<'a>(&self, cx: &'a App) -> &'a DeviceProfile {
        &cx.global::<AppState>().settings.device.profile
    }

    /// Settings → form.
    pub fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = self.profile(cx).clone();
        self.shown = p.clone();
        self.loading = true;
        let set = |e: &Entity<InputState>, v: String, window: &mut Window, cx: &mut Context<Self>| e.update(cx, |s, cx| s.set_value(v, window, cx));
        set(&self.name, p.name.clone(), window, cx);
        set(&self.custom_up, p.pen_up.clone(), window, cx);
        set(&self.custom_down, p.pen_down.clone(), window, cx);
        for (f, v) in [
            (F::WriteSpeed, p.write_speed),
            (F::TravelSpeed, p.travel_speed),
            (F::Pause, p.pause_after_down_s),
            (F::UpZ, p.pen_up_z),
            (F::DownZ, p.pen_down_z),
            (F::PressStep, p.press_step),
        ] {
            let e = self.input(f);
            set(&e, fmt_num(v), window, cx);
        }
        self.loading = false;
        cx.notify();
    }

    /// Form → settings (invalid fields keep their previous value).
    fn read_form(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let num = |f: F, cx: &App| self.input(f).read(cx).value().trim().parse::<f32>().ok().filter(|v| v.is_finite());
        let text = |e: &Entity<InputState>, cx: &App| e.read(cx).value().trim().to_string();
        let mut p = self.profile(cx).clone();
        let name = text(&self.name, cx);
        if !name.is_empty() {
            p.name = name;
        }
        if let Some(v) = num(F::WriteSpeed, cx).filter(|v| *v >= 100.0) {
            p.write_speed = v;
        }
        if let Some(v) = num(F::TravelSpeed, cx).filter(|v| *v >= 100.0) {
            p.travel_speed = v;
        }
        if let Some(v) = num(F::Pause, cx) {
            p.pause_after_down_s = v.clamp(0.0, 10.0);
        }
        if let Some(v) = num(F::UpZ, cx) {
            p.pen_up_z = v;
        }
        if let Some(v) = num(F::DownZ, cx) {
            p.pen_down_z = v;
        }
        if let Some(v) = num(F::PressStep, cx) {
            p.press_step = v.clamp(0.0, 5.0);
        }
        let (up, down) = (text(&self.custom_up, cx), text(&self.custom_down, cx));
        if !up.is_empty() {
            p.pen_up = up;
        }
        if !down.is_empty() {
            p.pen_down = down;
        }
        self.store(p, cx);
    }

    fn store(&mut self, p: DeviceProfile, cx: &mut Context<Self>) {
        if &p == self.profile(cx) {
            return;
        }
        self.handle.send(Command::SetProfile(p.clone()));
        self.shown = p.clone();
        let st = cx.global_mut::<AppState>();
        st.settings.device.profile = p;
        st.save_settings();
        cx.notify();
    }

    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut DeviceProfile)) {
        let mut p = self.profile(cx).clone();
        f(&mut p);
        self.store(p, cx);
    }

    /// 从炽写配置导入: ChiXie3's own `config.ini`.
    fn import_ini(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_default().join("ChiXie3/conf/config.ini");
        let res = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| chixie3_devices::import(&String::from_utf8_lossy(&b)).map_err(|e| e.to_string()));
        self.offer(res, &path, window, cx);
    }

    /// 从 .cset 文件导入: pick a ChiXie3 device-parameter file. The picker cannot filter by
    /// extension, so a file that is not one fails to read with an error notice.
    fn import_cset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("选择炽写设备参数文件（*.cset）".into()) });
        window
            .spawn(cx, async move |cx| {
                let Ok(Ok(Some(paths))) = rx.await else { return };
                let Some(path) = paths.into_iter().next() else { return };
                let _ = cx.update(|w, cx| view.update(cx, |v, cx| v.load_cset(&path, w, cx)));
            })
            .detach();
    }

    fn load_cset(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let res = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| chixie3_devices::import_cset(&b).map_err(|e| e.to_string()));
        self.offer(res, path, window, cx);
    }

    /// Apply the devices read from `path`: one goes straight in, several let the user choose.
    fn offer(&mut self, res: Result<Vec<DeviceProfile>, String>, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        match res {
            Ok(mut ds) if ds.len() == 1 => self.adopt(ds.remove(0), window, cx),
            Ok(ds) if ds.is_empty() => window.push_notification(Notification::warning(format!("{} 里没有设备参数", path.display())), cx),
            Ok(ds) => self.choose(ds, window, cx),
            Err(e) => window.push_notification(Notification::error(format!("读取 {} 失败：{e}", path.display())), cx),
        }
    }

    /// Several devices in the file: a dialog lists them and the picked one is applied.
    fn choose(&self, ds: Vec<DeviceProfile>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_dialog(cx, move |d, _w, _cx| {
            let rows = ds.iter().enumerate().map(|(i, p)| {
                let (view, p) = (view.clone(), p.clone());
                Button::new(format!("pick-device-{i}")).w_full().label(p.name.clone()).on_click(move |_: &ClickEvent, w: &mut Window, cx: &mut App| {
                    w.close_dialog(cx);
                    view.update(cx, |v, cx| v.adopt(p.clone(), w, cx));
                })
            });
            d.title("选择要导入的设备").w(px(380.)).child(v_flex().gap_2().children(rows))
        });
    }

    /// Make `d` this device's profile and show it.
    fn adopt(&mut self, d: DeviceProfile, window: &mut Window, cx: &mut Context<Self>) {
        let name = d.name.clone();
        self.store(d, cx);
        self.load(window, cx);
        window.push_notification(Notification::success(format!("已导入炽写设备「{name}」的参数")), cx);
    }

    #[inline(never)]
    /// 书写优化: 优化规则 and its strength (简化 tolerance / 扩张数量 / point spacing).
    fn route_menu(&self, p: &DeviceProfile, cx: &mut Context<Self>) -> AnyElement {
        use chixie_rs_core::path::{RouteImprove, RouteMode};
        let route = p.route;
        let this = cx.entity().downgrade();
        let w1 = this.clone();
        let mode = Button::new("route-mode").small().outline().w_full().label(route.mode.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            RouteMode::ALL.iter().fold(m, |m, &mode| {
                let this = w1.clone();
                m.item(PopupMenuItem::new(mode.label()).checked(mode == route.mode).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.route = RouteImprove { mode, value: mode.default_value() }));
                }))
            })
        });
        let (presets, unit): (&[f32], &str) = match route.mode {
            RouteMode::None => (&[], ""),
            RouteMode::Simplify => (&[0.01, 0.02, 0.05, 0.1], "mm 误差"),
            RouteMode::CatmullRom => (&[2.0, 4.0, 8.0], "扩张数量"),
            RouteMode::Uniform => (&[0.1, 0.2, 0.5], "mm 间距"),
        };
        let w2 = this.clone();
        let strength = (!presets.is_empty()).then(|| {
            Button::new("route-value").small().outline().label(format!("{} {unit}", fmt_num(route.value))).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
                presets.iter().fold(m, |m, &value| {
                    let this = w2.clone();
                    m.item(PopupMenuItem::new(format!("{} {unit}", fmt_num(value))).checked(value == route.value).on_click(move |_, _w, cx| {
                        let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.route.value = value));
                    }))
                })
            })
        });
        h_flex().gap_2().child(div().flex_1().child(mode)).children(strength).into_any_element()
    }

    /// Change the page's output settings (页面旋转, 书写原点). They don't move the layout.
    fn edit_page(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut chixie_rs_core::config::PageSettings)) {
        let st = cx.global_mut::<AppState>();
        f(&mut st.settings.page);
        st.save_settings();
        cx.notify();
    }

    #[inline(never)]
    /// 路径规划, 页面旋转 and 书写原点 as dropdowns.
    fn output_menus(&self, p: &DeviceProfile, cx: &mut Context<Self>) -> [AnyElement; 3] {
        let page = &cx.global::<AppState>().settings.page;
        let (rotation, origin, plan) = (page.rotation, page.origin, p.path_plan);
        let this = cx.entity().downgrade();
        let w1 = this.clone();
        let plan_el = Button::new("path-plan").small().outline().w_full().label(plan.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            PathPlan::ALL.iter().fold(m, |m, &k| {
                let this = w1.clone();
                m.item(PopupMenuItem::new(k.label()).checked(k == plan).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.path_plan = k));
                }))
            })
        });
        let w2 = this.clone();
        let rot_el = Button::new("rotation").small().outline().w_full().label(rotation.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            PageRotation::ALL.iter().fold(m, |m, &r| {
                let this = w2.clone();
                m.item(PopupMenuItem::new(r.label()).checked(r == rotation).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit_page(cx, |s| s.rotation = r));
                }))
            })
        });
        let w3 = this.clone();
        let mut choices = vec![Origin::Paper, Origin::Frame];
        if matches!(origin, Origin::Custom { .. }) {
            choices.push(origin);
        }
        let origin_el = Button::new("origin").small().outline().w_full().label(origin.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            choices.iter().fold(m, |m, &o| {
                let this = w3.clone();
                m.item(PopupMenuItem::new(o.label()).checked(o == origin).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit_page(cx, |s| s.origin = o));
                }))
            })
        });
        [plan_el.into_any_element(), rot_el.into_any_element(), origin_el.into_any_element()]
    }

    #[inline(never)]
    /// 笔锋 (experimental): tail length; the pen rises halfway to its up height.
    fn taper_menu(&self, p: &DeviceProfile, cx: &mut Context<Self>) -> AnyElement {
        use chixie_rs_core::profile::TipTaper;
        let cur = p.tip_taper.map(|t| t.len_mm);
        let this = cx.entity().downgrade();
        let label = cur.map_or("关闭".to_string(), |l| format!("末端 {} mm 渐抬", fmt_num(l)));
        Button::new("taper")
            .small()
            .outline()
            .w_full()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |m, _w, _cx| {
                [None, Some(1.0), Some(2.0), Some(4.0)].into_iter().fold(m, |m, len: Option<f32>| {
                    let this = this.clone();
                    let name = len.map_or("关闭".to_string(), |l| format!("末端 {} mm 渐抬", fmt_num(l)));
                    m.item(PopupMenuItem::new(name).checked(len == cur).on_click(move |_, _w, cx| {
                        let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.tip_taper = len.map(|len_mm| TipTaper { len_mm, z_end: (p.pen_up_z + p.pen_down_z) / 2.0 })));
                    }))
                })
            })
            .into_any_element()
    }

    #[inline(never)]
    /// 多色绘制 on/off and 换笔模式. The automatic changer's pens (macros) live in the settings
    /// file; the form shows how many there are.
    fn multicolour_rows(&self, p: &DeviceProfile, cx: &mut Context<Self>) -> AnyElement {
        use chixie_rs_core::profile::PenChange;
        let muted = cx.theme().muted_foreground;
        let on = cx.global::<AppState>().settings.page.multicolour;
        let mode = p.pen_change.clone();
        let auto = matches!(mode, PenChange::Auto { .. });
        let slots = match &mode {
            PenChange::Auto { slots } => slots.len(),
            PenChange::Manual => 0,
        };
        let this = cx.entity().downgrade();
        let mode_el = Button::new("pen-change").small().outline().w_full().label(if auto { "自动换笔（需设备支持）" } else { "手动换笔（弹窗提示）" }).dropdown_caret(true).dropdown_menu(
            move |m, _w, _cx| {
                let (a, b) = (this.clone(), this.clone());
                m.item(PopupMenuItem::new("手动换笔（弹窗提示）").checked(!auto).on_click(move |_, _w, cx| {
                    let _ = a.update(cx, |v, cx| v.edit(cx, |p| p.pen_change = PenChange::Manual));
                }))
                .item(PopupMenuItem::new("自动换笔（需设备支持）").checked(auto).on_click(move |_, _w, cx| {
                    let _ = b.update(cx, |v, cx| {
                        v.edit(cx, |p| {
                            if !matches!(p.pen_change, PenChange::Auto { .. }) {
                                p.pen_change = PenChange::Auto { slots: Vec::new() };
                            }
                        })
                    });
                }))
            },
        );
        v_flex()
            .gap_1p5()
            .child(Checkbox::new("multicolour").small().label("多色绘制（按笔色分组书写，换色时换笔）").checked(on).on_click(cx.listener(|v, on: &bool, _w, cx| {
                let on = *on;
                v.edit_page(cx, |s| s.multicolour = on)
            })))
            .when(on, |el| {
                el.child(h_flex().gap_2().child(div().w(px(76.)).flex_none().text_sm().text_color(muted).child("换笔模式")).child(div().flex_1().child(mode_el)))
                    .when(auto, |el| {
                        el.child(div().text_xs().text_color(muted).child(format!(
                            "自动换笔配置：共{slots}支笔（在设置文件 [device.profile.pen_change] 的 slots 里填写各笔颜色与取笔/放笔 G-code；没有配置的颜色会弹窗提示手动换笔）"
                        )))
                    })
            })
            .into_any_element()
    }

    fn form(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.profile(cx).clone();
        let theme = cx.theme();
        let (muted, warn) = (theme.muted_foreground, theme.warning);
        let this = cx.entity().downgrade();
        let label = |s: &'static str| div().w(px(76.)).flex_none().text_sm().text_color(muted).child(s);
        let row = |l: &'static str, el: AnyElement| h_flex().gap_2().child(label(l)).child(div().flex_1().child(el));

        let w1 = this.clone();
        let fw = Button::new("fw").small().outline().w_full().label(p.firmware.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            Firmware::ALL.iter().fold(m, |m, f| {
                let (this, f) = (w1.clone(), *f);
                m.item(PopupMenuItem::new(f.label()).checked(f == p.firmware).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.firmware = f));
                }))
            })
        });
        let w2 = this.clone();
        let dt = Button::new("dtype").small().outline().w_full().label(p.device_type.label()).dropdown_caret(true).dropdown_menu(move |m, _w, _cx| {
            DeviceType::ALL.iter().fold(m, |m, t| {
                let (this, t) = (w2.clone(), *t);
                m.item(PopupMenuItem::new(t.label()).checked(t == p.device_type).on_click(move |_, _w, cx| {
                    let _ = this.update(cx, |v, cx| v.edit(cx, |p| p.device_type = t));
                }))
            })
        });
        let servo = p.device_type == DeviceType::ServoPenUp;
        let num = |f: F| {
            let (_, l, unit, ..) = NUMS.iter().find(|n| n.0 == f).copied().expect("field");
            // A servo's pen position is an angle (ChiXie3: 落笔角度 / 度), still sent as Z.
            let (l, unit) = match (servo, f) {
                (true, F::UpZ) => ("抬笔角度", "度"),
                (true, F::DownZ) => ("落笔角度", "度"),
                (true, F::PressStep) => (l, "度"),
                _ => (l, unit),
            };
            row(l, NumberInput::new(&self.input(f)).small().suffix(unit).into_any_element())
        };
        let max_feed = self.machine.as_ref().and_then(MachineSettings::max_feed);
        let too_fast = max_feed.filter(|m| p.write_speed > *m || p.travel_speed > *m);
        let custom = p.device_type == DeviceType::CustomZ;
        let am = p.axis_map;
        let [plan_el, rot_el, origin_el] = self.output_menus(&p, cx);
        let turned = cx.global::<AppState>().settings.page.rotation != PageRotation::None;

        v_flex()
            .gap_1p5()
            .child(row("设备名", Input::new(&self.name).small().into_any_element()))
            .child(row("固件", fw.into_any_element()))
            .when(!p.firmware.supported(), |el| el.child(div().text_xs().text_color(warn).child("Marlin 固件暂不支持书写，请选择 Grbl 系列。")))
            .child(row("设备类型", dt.into_any_element()))
            .child(num(F::WriteSpeed))
            .child(num(F::TravelSpeed))
            .when_some(too_fast, |el, m| {
                el.child(div().text_xs().text_color(warn).child(format!("超过机器最大速度 {m:.0} mm/min（$110/$111），固件会按最大速度运行。")))
            })
            .child(num(F::Pause))
            .when(!custom, |el| el.child(num(F::UpZ)).child(num(F::DownZ)))
            .when(matches!(p.device_type, DeviceType::StepperPenUp | DeviceType::ServoPenUp), |el| el.child(num(F::PressStep)))
            .when(custom, |el| {
                el.child(row("抬笔指令", Input::new(&self.custom_up).small().into_any_element()))
                    .child(row("落笔指令", Input::new(&self.custom_down).small().into_any_element()))
            })
            .child(div().text_xs().text_color(muted).child(format!("抬笔：{}　落笔：{}", p.pen_up_cmd(), p.pen_down_cmd())))
            .child(row("书写优化", self.route_menu(&p, cx)))
            .when(p.device_type == DeviceType::StepperPenUp, |el| el.child(row("笔锋(实验)", self.taper_menu(&p, cx))))
            .child(row("路径规划", plan_el))
            .child(row("书写原点", origin_el))
            .child(row("页面旋转", rot_el))
            .when(turned, |el| el.child(div().text_xs().text_color(muted).child("页面预览不旋转，只旋转书写输出；转过的页面原点仍在纸张左上角。")))
            .child(self.multicolour_rows(&p, cx))
            .child(div().pt_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("坐标方向"))
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(Checkbox::new("swap").small().label("交换 X/Y").checked(am.swap_xy).on_click(cx.listener(|v, on: &bool, _w, cx| {
                        let on = *on;
                        v.edit(cx, |p| p.axis_map.swap_xy = on)
                    })))
                    .child(Checkbox::new("invx").small().label("X 反向").checked(am.invert_x).on_click(cx.listener(|v, on: &bool, _w, cx| {
                        let on = *on;
                        v.edit(cx, |p| p.axis_map.invert_x = on)
                    })))
                    .child(Checkbox::new("invy").small().label("Y 反向").checked(am.invert_y).on_click(cx.listener(|v, on: &bool, _w, cx| {
                        let on = *on;
                        v.edit(cx, |p| p.axis_map.invert_y = on)
                    }))),
            )
            .child(
                h_flex()
                    .pt_2()
                    .gap_2()
                    .flex_wrap()
                    .child(Button::new("p-down").small().label("测试落笔").on_click(cx.listener(|v, _: &ClickEvent, _w, _cx| v.handle.send(Command::Pen(Pen::Down)))))
                    .child(Button::new("p-up").small().label("测试抬笔").on_click(cx.listener(|v, _: &ClickEvent, _w, _cx| v.handle.send(Command::Pen(Pen::Up)))))
                    .child({
                        let this = cx.entity();
                        Button::new("p-import")
                            .small()
                            .icon(Icon::new(IconName::Import))
                            .label("从炽写导入")
                            .dropdown_caret(true)
                            .dropdown_menu(move |m, _w, _cx| {
                                let (a, b) = (this.clone(), this.clone());
                                m.item(PopupMenuItem::new("炽写配置 (config.ini)").on_click(move |_, w, cx| a.update(cx, |v, cx| v.import_ini(w, cx))))
                                    .item(PopupMenuItem::new("设备参数文件 (*.cset)…").on_click(move |_, w, cx| b.update(cx, |v, cx| v.import_cset(w, cx))))
                            })
                    }),
            )
            .into_any_element()
    }

    #[inline(never)]
    fn specs(&self, connected: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, bg) = (theme.muted_foreground, theme.border, theme.muted);
        let body: AnyElement = match &self.machine {
            None => div()
                .text_xs()
                .text_color(muted)
                .child(if connected { "读取中…" } else { "连接设备后显示（读取 $$）" })
                .into_any_element(),
            Some(m) => {
                let pair = |v: Option<(f32, f32)>, unit: &str| match v {
                    Some((x, y)) if x == y => format!("{x:.0} {unit}"),
                    Some((x, y)) => format!("X {x:.0} / Y {y:.0} {unit}"),
                    None => "—".into(),
                };
                let line = |k: &'static str, v: String| h_flex().gap_2().child(div().w(px(76.)).text_color(muted).child(k)).child(v);
                v_flex()
                    .gap_0p5()
                    .text_sm()
                    .child(line("行程", pair(m.travel, "mm")))
                    .child(line("最大速度", pair(m.max_rate, "mm/min")))
                    .child(line("加速度", pair(m.accel, "mm/s²")))
                    .child(line("步数", pair(m.steps_per_mm, "步/mm")))
                    .into_any_element()
            }
        };
        v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                h_flex()
                    .child(div().flex_1().text_sm().font_weight(FontWeight::SEMIBOLD).child("机器规格"))
                    .child(
                        Button::new("specs-refresh")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::RefreshCw))
                            .disabled(!connected)
                            .tooltip("重新读取 $$")
                            .on_click(cx.listener(|v, _: &ClickEvent, _w, _cx| v.handle.send(Command::QueryMachine))),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_with(&self, connected: bool, cx: &mut Context<Self>) -> AnyElement {
        v_flex().gap_3().child(self.specs(connected, cx)).child(self.form(cx)).into_any_element()
    }
}

impl Render for DeviceParams {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = self.connected;
        self.render_with(connected, cx)
    }
}
