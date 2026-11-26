//! Device settings: how the machine lifts the pen, its feeds, axes and where it parks. The
//! G-code itself is written by `penkaku-output`.

use serde::{Deserialize, Serialize};

/// Controller firmware family (ChiXie3 `firmware_type`). Only Grbl variants stream today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Firmware {
    /// `ChiXie`: classic Grbl board.
    Grbl,
    /// `ESP32_Grbl`.
    Esp32Grbl,
    /// `ChiXie_ESP32_Grbl` (the verified board: Grbl_ESP32 1.3a).
    #[default]
    ChiXieEsp32Grbl,
    /// `Marlin`: not supported for streaming yet.
    Marlin,
}

impl Firmware {
    pub const ALL: [Firmware; 4] = [Firmware::Grbl, Firmware::Esp32Grbl, Firmware::ChiXieEsp32Grbl, Firmware::Marlin];
    pub fn label(self) -> &'static str {
        match self {
            Firmware::Grbl => "ChiXie (Grbl)",
            Firmware::Esp32Grbl => "ESP32_Grbl",
            Firmware::ChiXieEsp32Grbl => "ChiXie_ESP32_Grbl",
            Firmware::Marlin => "Marlin（暂不支持）",
        }
    }
    pub fn supported(self) -> bool {
        self != Firmware::Marlin
    }
}

/// How the pen is lifted (ChiXie3 `DeviceType`). The writing robots all lift with an absolute Z
/// move; Grbl_ESP32 maps Z to a stepper, an RC servo or a solenoid in firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// 步进抬笔写字机 (`WritingRobot_StepperPenUp`).
    #[default]
    StepperPenUp,
    /// 舵机抬笔写字机 (`WritingRobot_SteeringEnginePenUp`).
    ServoPenUp,
    /// 电磁铁抬笔写字机 (`WritingRobot_ElectroMagnetPenUp`).
    ElectromagnetPenUp,
    /// 自定义 Z 指令: `pen_up` / `pen_down` are sent verbatim.
    CustomZ,
}

impl DeviceType {
    pub const ALL: [DeviceType; 4] = [DeviceType::StepperPenUp, DeviceType::ServoPenUp, DeviceType::ElectromagnetPenUp, DeviceType::CustomZ];
    pub fn label(self) -> &'static str {
        match self {
            DeviceType::StepperPenUp => "步进抬笔写字机",
            DeviceType::ServoPenUp => "舵机抬笔写字机",
            DeviceType::ElectromagnetPenUp => "电磁铁抬笔写字机",
            DeviceType::CustomZ => "自定义抬落笔指令",
        }
    }
}

/// Logical → machine axis mapping (ChiXie3 `x_plus` / `y_plus` …). Identity by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AxisMap {
    /// Logical X drives machine Y (and Y drives X).
    pub swap_xy: bool,
    /// Negate the (post-swap) machine X / Y.
    pub invert_x: bool,
    pub invert_y: bool,
}

impl AxisMap {
    pub fn is_identity(&self) -> bool {
        *self == AxisMap::default()
    }
    pub fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (mut x, mut y) = if self.swap_xy { (y, x) } else { (x, y) };
        if self.invert_x {
            x = -x;
        }
        if self.invert_y {
            y = -y;
        }
        (x, y)
    }
}

/// After a page the pen lifts and rapids back to the work origin (the zero set before the job),
/// ready for the next sheet. ChiXie3's 写完位置 choice is not kept.
pub const PARK: &str = "G90G0 X0Y0";

/// How the device moves the pen. Defaults match the verified Grbl_ESP32 board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceProfile {
    /// 设备名.
    pub name: String,
    pub firmware: Firmware,
    pub device_type: DeviceType,
    /// Pen positions for the Z-lift device types: `G1G90 Z<up>` / `G1G90 Z<down>`. mm for a stepper;
    /// for a servo ChiXie3 calls it 落笔角度 (degrees) and still sends `G1G90 Z<value>`.
    pub pen_up_z: f32,
    pub pen_down_z: f32,
    /// Raw pen commands, used only by `DeviceType::CustomZ`.
    pub pen_up: String,
    pub pen_down: String,
    /// Writing feed (pen down) and travel feed (pen up), mm/min. ChiXie3 uses one 写字速度 for
    /// both; the firmware clamps to its max rate ($110/$111).
    pub write_speed: f32,
    pub travel_speed: f32,
    /// 落笔后暂停 (s): dwell (`G4 P<s>`) after each pen-down so the pen settles. 0 = none.
    /// ChiXie3 3.0.310 has the same setting but sent no dwell (and no delay) over Telnet with
    /// 0.5 s for both stepper and servo types (captured 2026-11-07); ours is a real dwell.
    pub pause_after_down_s: f32,
    pub axis_map: AxisMap,
    /// 书写优化: how each stroke is reshaped before it is written.
    pub route: crate::path::RouteImprove,
    /// 路径规划: the order rows are written in.
    pub path_plan: PathPlan,
    /// 换笔模式 for 多色绘制.
    pub pen_change: PenChange,
    /// 笔锋 (experimental, stepper lift only): lift the pen gradually over each stroke's tail.
    pub tip_taper: Option<TipTaper>,
    /// 字符轻重: pen-down Z moves this much per step (更轻Ⅰ = −1 step). 0 = ignore 轻重.
    pub press_step: f32,
}

/// 笔锋: over the last `len_mm` of a stroke, Z moves linearly from `pen_down_z` to `z_end`, so a
/// soft pen thins out like a brush tip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TipTaper {
    pub len_mm: f32,
    pub z_end: f32,
}

/// Pen colour, RGB. Text without a colour is written in [`BLACK`].
pub type Rgb = [u8; 3];
pub const BLACK: Rgb = [0, 0, 0];

/// 换笔模式 (ChiXie3 `PenChangingMode`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PenChange {
    /// 手动换笔（弹窗提示）: the job pauses and asks for the next colour.
    #[default]
    Manual,
    /// 自动换笔（需设备支持）: G-code macros put the old pen back and pick the new one.
    Auto { slots: Vec<PenSlot> },
}

/// One pen of an automatic changer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PenSlot {
    pub colour: Rgb,
    /// G-code that picks this pen up / puts it back (one command per line).
    pub pick: String,
    pub drop: String,
}

/// A colour's name for prompts (common pens by name, else `#RRGGBB`).
pub fn colour_name(c: Rgb) -> String {
    match c {
        [0, 0, 0] => "黑色".into(),
        [255, 0, 0] | [200, 0, 0] | [192, 0, 0] => "红色".into(),
        [0, 0, 255] | [0, 112, 192] => "蓝色".into(),
        [0, 128, 0] | [0, 176, 80] => "绿色".into(),
        [c0, c1, c2] => format!("#{c0:02X}{c1:02X}{c2:02X}"),
    }
}

/// 路径规划 (ChiXie3 `path_planning_type`). Output order only; the layout is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPlan {
    /// 之字型(默认): every row left to right.
    #[default]
    Raster,
    /// 弓字型: every second row right to left (each character's own strokes keep their order).
    Serpentine,
    /// 自动选择最近路径: each row starts from whichever end is nearer to where the pen stopped.
    Nearest,
}

impl PathPlan {
    pub const ALL: [PathPlan; 3] = [PathPlan::Raster, PathPlan::Serpentine, PathPlan::Nearest];
    pub fn label(self) -> &'static str {
        match self {
            PathPlan::Raster => "之字形：永远左起(默认)",
            PathPlan::Serpentine => "弓字形：左右左(蛇形)",
            PathPlan::Nearest => "自动选择最近路径",
        }
    }
}

/// 书写原点 (ChiXie3 `start_point_mode`): where the pen stands when the job starts.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// 纸张起点: the paper's top-left corner.
    Paper,
    /// 框线起点: the top-left of the writable frame (inside the margins). Today's behaviour.
    #[default]
    Frame,
    /// 自定义: a point in paper mm from the top-left corner (x right, y down).
    Custom { x: f32, y: f32 },
}

impl Origin {
    pub fn label(self) -> String {
        match self {
            Origin::Paper => "纸张起点".into(),
            Origin::Frame => "框线起点".into(),
            Origin::Custom { x, y } => format!("自定义 ({x:.0}, {y:.0}) mm"),
        }
    }
}

/// 页面旋转 (ChiXie3 `ParityPageDirectionMode`): write some pages turned 180°, for the back of a
/// sheet or a feeder that returns sheets upside down. The preview is not rotated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageRotation {
    /// 不旋转.
    #[default]
    None,
    /// 奇数页旋转180° (pages 1, 3, …).
    Odd180,
    /// 偶数页旋转180° (pages 2, 4, …).
    Even180,
    /// 旋转180°: every page.
    All180,
}

impl PageRotation {
    pub const ALL: [PageRotation; 4] = [PageRotation::None, PageRotation::Odd180, PageRotation::Even180, PageRotation::All180];
    pub fn label(self) -> &'static str {
        match self {
            PageRotation::None => "不旋转",
            PageRotation::Odd180 => "奇数页旋转180°",
            PageRotation::Even180 => "偶数页旋转180°",
            PageRotation::All180 => "旋转180°",
        }
    }
    /// Whether document page `k` (0-based) is turned.
    pub fn turns(self, k: usize) -> bool {
        match self {
            PageRotation::None => false,
            PageRotation::Odd180 => k.is_multiple_of(2),
            PageRotation::Even180 => !k.is_multiple_of(2),
            PageRotation::All180 => true,
        }
    }
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            name: "赤写写字机".into(),
            firmware: Firmware::default(),
            device_type: DeviceType::default(),
            pen_up_z: 0.0,
            pen_down_z: 2.0,
            pen_up: "G1G90 Z0".into(),
            pen_down: "G1G90 Z2".into(),
            write_speed: 3000.0,
            travel_speed: 8000.0,
            pause_after_down_s: 0.0,
            axis_map: AxisMap::default(),
            route: Default::default(),
            path_plan: PathPlan::default(),
            pen_change: PenChange::default(),
            tip_taper: None,
            press_step: PRESS_STEP,
        }
    }
}

/// Default Z per 字符轻重 step (mm for a stepper lift). A guess; tune per pen.
pub const PRESS_STEP: f32 = 0.2;

/// `2.0` → "2", `1.25` → "1.25" (keeps the historical `Z0` / `Z2` literals byte-identical).
pub fn num(v: f32) -> String {
    let s = format!("{:.3}", v + 0.0); // + 0.0 turns -0.0 into 0.0
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl DeviceProfile {
    pub fn pen_up_cmd(&self) -> String {
        match self.device_type {
            DeviceType::CustomZ => self.pen_up.clone(),
            _ => format!("G1G90 Z{}", num(self.pen_up_z)),
        }
    }
    pub fn pen_down_cmd(&self) -> String {
        match self.device_type {
            DeviceType::CustomZ => self.pen_down.clone(),
            _ => format!("G1G90 Z{}", num(self.pen_down_z)),
        }
    }

    /// 字符轻重 moves the pen-down Z: stepper and servo lifts only.
    pub fn supports_press(&self) -> bool {
        matches!(self.device_type, DeviceType::StepperPenUp | DeviceType::ServoPenUp) && self.press_step != 0.0
    }

    /// 落笔校准 depths: 11 pen-down Z values around today's, 0.2 mm apart (3° on a servo),
    /// shallow to deep, leaving out any at or behind the pen-up position.
    pub fn ladder_levels(&self) -> Vec<f32> {
        let away = if self.pen_down_z >= self.pen_up_z { 1.0 } else { -1.0 };
        let step = if self.device_type == DeviceType::ServoPenUp { 3.0 } else { 0.2 };
        (-5..=5).map(|k| self.pen_down_z + away * k as f32 * step).filter(|z| (z - self.pen_up_z) * away > 1e-4).map(|z| (z * 1000.0).round() / 1000.0).collect()
    }

    /// Pen-down Z at `press` 轻重 steps: heavier goes further from the pen-up position.
    pub fn pen_down_z_at(&self, press: f32) -> f32 {
        let away = if self.pen_down_z >= self.pen_up_z { 1.0 } else { -1.0 };
        self.pen_down_z + away * press * self.press_step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_levels_go_shallow_to_deep_and_stay_down() {
        let d = DeviceProfile::default(); // up 0, down 2
        assert_eq!(d.ladder_levels(), [1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.2, 2.4, 2.6, 2.8, 3.0]);
        // Close to the pen-up position: the levels at or above it are left out.
        let near = DeviceProfile { pen_down_z: 0.6, ..Default::default() };
        assert_eq!(near.ladder_levels(), [0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6]);
        // Down below up: deeper is lower.
        let inv = DeviceProfile { pen_up_z: 5.0, pen_down_z: 2.0, ..Default::default() };
        assert_eq!(inv.ladder_levels().first(), Some(&3.0));
        assert_eq!(inv.ladder_levels().last(), Some(&1.0));
        let servo = DeviceProfile { device_type: DeviceType::ServoPenUp, pen_up_z: 30.0, pen_down_z: 60.0, ..Default::default() };
        assert_eq!(servo.ladder_levels()[..2], [45.0, 48.0]);
    }

    #[test]
    fn settings_with_the_old_end_position_still_load() {
        let old: DeviceProfile = toml::from_str("write_speed = 2000.0\n[end_position]\nkind = \"origin_left\"\nmm = 20.0\n").unwrap();
        assert_eq!(old.write_speed, 2000.0);
    }

    #[test]
    fn old_settings_without_new_fields_still_load() {
        let old = "pen_up = \"G1G90 Z0\"\npen_down = \"G1G90 Z2\"\nwrite_speed = 2500.0\ntravel_speed = 8000.0\n";
        let d: DeviceProfile = toml::from_str(old).unwrap();
        assert_eq!(d.write_speed, 2500.0);
        assert_eq!((d.device_type, d.pen_down_cmd()), (DeviceType::StepperPenUp, "G1G90 Z2".to_string()));
    }
}
