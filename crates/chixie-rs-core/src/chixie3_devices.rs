//! Import ChiXie3 device parameters: `config.ini` `[DeviceConfig] DeviceParams`, and `.cset` files.
//!
//! QSettings stores the value as `@Variant(<bytes>)`: a QDataStream-serialised QVariantMap
//! `{device name: {key: value}}`, with the bytes escaped ini-style (`\0`, `\xHH`, `\b`, …). Only
//! the QVariant types ChiXie3 writes are decoded (map, string, int, bool, double).
//!
//! A `.cset` is a Python pickle of the same `{device name: {key: value}}` dict (see `pickle`).

use crate::pickle;
use crate::profile::{AxisMap, DeviceProfile, DeviceType, EndPosition, Firmware};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Map(Vec<(String, Value)>),
    Str(String),
    Int(i64),
    Bool(bool),
    Double(f64),
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Double(d) => Some(*d),
            Value::Str(s) => s.trim().parse().ok(),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            _ => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Every device in a ChiXie3 `config.ini`, as profiles (empty if there is no `DeviceParams`).
pub fn import(ini_text: &str) -> Result<Vec<DeviceProfile>> {
    let Some(raw) = ini_text.lines().find_map(|l| l.trim().strip_prefix("DeviceParams=")) else { return Ok(Vec::new()) };
    let inner = raw.strip_prefix("@Variant(").and_then(|r| r.strip_suffix(')')).ok_or_else(|| bad("DeviceParams is not a @Variant"))?;
    let bytes = unescape(inner)?;
    let mut r = Reader { b: &bytes, i: 0 };
    let Value::Map(devices) = r.variant()? else { return Err(bad("DeviceParams is not a map")) };
    Ok(devices.iter().map(|(name, params)| profile(name, params)).collect())
}

/// Every device in a `.cset` file, as profiles. Parsed as data only: nothing in it is executed.
pub fn import_cset(bytes: &[u8]) -> Result<Vec<DeviceProfile>> {
    let pickle::Value::Dict(devices) = pickle::parse(bytes)? else { return Err(bad(".cset is not a dict")) };
    devices
        .iter()
        .map(|(name, params)| -> Result<DeviceProfile> {
            let name = name.as_str().ok_or_else(|| bad("device name is not a string"))?;
            match from_pickle(params) {
                Some(p @ Value::Map(_)) => Ok(profile(name, &p)),
                _ => Err(bad(&format!("{name}: parameters are not a dict"))),
            }
        })
        .collect()
}

/// A `.cset` value in the shape `profile` reads. Entries of other kinds (lists, objects, None) are
/// dropped, as an unknown key would be.
fn from_pickle(v: &pickle::Value) -> Option<Value> {
    Some(match v {
        pickle::Value::Bool(b) => Value::Bool(*b),
        pickle::Value::Int(i) => Value::Int(*i),
        pickle::Value::Float(f) => Value::Double(*f),
        pickle::Value::Str(s) => Value::Str(s.clone()),
        pickle::Value::Dict(items) => Value::Map(items.iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), from_pickle(v)?))).collect()),
        _ => return None,
    })
}

fn profile(name: &str, p: &Value) -> DeviceProfile {
    let num = |k: &str| p.get(k).and_then(Value::num);
    let mut d = DeviceProfile { name: name.to_string(), ..Default::default() };
    if let Some(t) = num("deviceType") {
        d.device_type = device_type(t as i64);
    }
    if let Some(f) = num("firmware") {
        d.firmware = firmware(f as i64);
    }
    if let Some(v) = num("penUp") {
        d.pen_up_z = v as f32;
    }
    if let Some(v) = num("penDown") {
        d.pen_down_z = v as f32;
    }
    // ChiXie3 has one 写字速度 for every move.
    if let Some(v) = num("writeSpeed").filter(|v| *v > 0.0) {
        (d.write_speed, d.travel_speed) = (v as f32, v as f32);
    }
    if let Some(v) = num("pauseTime") {
        d.pause_after_down_s = v.max(0.0) as f32;
    }
    if let Some(v) = num("WriteDonePos") {
        d.end_position = end_position(v as i64);
    }
    // Each axis is labelled by its `+` side (`x_plus`); its `-` side (`x_reduce`) is the opposite
    // label, which stands in when the `+` one is missing.
    let dir = |k: &str| p.get(k).and_then(Value::text).unwrap_or_default().to_ascii_uppercase();
    let plus = |plus_key: &str, minus_key: &str| {
        let label = dir(plus_key);
        if label.is_empty() { opposite(&dir(minus_key)) } else { label }
    };
    let (xp, yp) = (plus("x_plus", "x_reduce"), plus("y_plus", "y_reduce"));
    let swap = xp.starts_with('Y');
    d.axis_map = AxisMap { swap_xy: swap, invert_x: xp.ends_with('-') && !swap || swap && yp.ends_with('-'), invert_y: if swap { xp.ends_with('-') } else { yp.ends_with('-') } };
    if xp.is_empty() && yp.is_empty() {
        d.axis_map = AxisMap::default();
    }
    d
}

/// ChiXie3 `DeviceType` drop-down order (captured 2026-11-07): 0 步进抬笔写字机, 1 舵机抬笔写字机,
/// 2 电磁铁抬笔写字机, 3 激光雕刻机, 4 3D打印机, 5 自定义抬落笔指令设备. Laser / 3D printer are not
/// supported and fall back to stepper.
fn device_type(i: i64) -> DeviceType {
    match i {
        1 => DeviceType::ServoPenUp,
        2 => DeviceType::ElectromagnetPenUp,
        5 => DeviceType::CustomZ,
        _ => DeviceType::StepperPenUp,
    }
}

/// ChiXie3 写完位置 (`WriteDonePos`) choices: 0 原点, 1 原点左侧2cm, 2 任务结束点.
fn end_position(i: i64) -> EndPosition {
    match i {
        1 => EndPosition::OriginLeft { mm: EndPosition::ORIGIN_LEFT_MM },
        2 => EndPosition::Stay,
        _ => EndPosition::Origin,
    }
}

/// `X+` ↔ `X-` and `Y+` ↔ `Y-`; empty for any other label.
fn opposite(label: &str) -> String {
    if let Some(axis) = label.strip_suffix('+') {
        format!("{axis}-")
    } else if let Some(axis) = label.strip_suffix('-') {
        format!("{axis}+")
    } else {
        String::new()
    }
}

/// ChiXie3 固件 drop-down order (captured 2026-11-07): 0 Grbl, 1 Marlin, 2 ESP32_Grbl,
/// 3 ChiXie_ESP32_Grbl.
fn firmware(i: i64) -> Firmware {
    match i {
        0 => Firmware::Grbl,
        1 => Firmware::Marlin,
        2 => Firmware::Esp32Grbl,
        _ => Firmware::ChiXieEsp32Grbl,
    }
}

fn bad(msg: &str) -> Error {
    Error::Config(format!("ChiXie3 DeviceParams: {msg}"))
}

/// Undo QSettings' ini escaping into raw bytes (each char is one Latin-1 byte).
fn unescape(s: &str) -> Result<Vec<u8>> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(c.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] != '\\' {
            out.push(u8::try_from(u32::from(c[i])).map_err(|_| bad("non-Latin-1 character"))?);
            i += 1;
            continue;
        }
        let e = *c.get(i + 1).ok_or_else(|| bad("trailing backslash"))?;
        i += 2;
        let simple = match e {
            'a' => Some(7),
            'b' => Some(8),
            'f' => Some(12),
            'n' => Some(10),
            'r' => Some(13),
            't' => Some(9),
            'v' => Some(11),
            '"' | '\'' | '?' | '\\' => Some(e as u8),
            _ => None,
        };
        if let Some(b) = simple {
            out.push(b);
        } else if e == 'x' {
            let start = i;
            while i < c.len() && c[i].is_ascii_hexdigit() && i - start < 4 {
                i += 1;
            }
            let v = u32::from_str_radix(&c[start..i].iter().collect::<String>(), 16).map_err(|_| bad("bad \\x escape"))?;
            out.push(u8::try_from(v).map_err(|_| bad("\\x escape above 0xFF"))?);
        } else if e.is_digit(8) {
            let start = i - 1;
            while i < c.len() && c[i].is_digit(8) && i - start < 3 {
                i += 1;
            }
            let v = u32::from_str_radix(&c[start..i].iter().collect::<String>(), 8).map_err(|_| bad("bad octal escape"))?;
            out.push(u8::try_from(v).map_err(|_| bad("octal escape above 0xFF"))?);
        } else {
            return Err(bad(&format!("unknown escape \\{e}")));
        }
    }
    Ok(out)
}

/// Big-endian QDataStream reader (Qt 5 format).
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.i.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| bad("truncated"))?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u32()?;
        if n == u32::MAX {
            return Ok(String::new()); // null QString
        }
        let units: Vec<u16> = self.take(n as usize)?.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        String::from_utf16(&units).map_err(|_| bad("invalid UTF-16"))
    }
    fn variant(&mut self) -> Result<Value> {
        // ChiXie3's stream has no per-variant "is null" byte: the type id is followed by the value.
        let ty = self.u32()?;
        Ok(match ty {
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::Int(i64::from(self.u32()? as i32)),
            3 => Value::Int(i64::from(self.u32()?)),
            6 => Value::Double(f64::from_be_bytes(self.take(8)?.try_into().expect("8 bytes"))),
            8 => {
                let n = self.u32()?;
                let mut m = Vec::new();
                for _ in 0..n {
                    let k = self.string()?;
                    m.push((k, self.variant()?));
                }
                Value::Map(m)
            }
            10 => Value::Str(self.string()?),
            // Unknown payload size: stop rather than misread the rest.
            other => return Err(bad(&format!("unsupported QVariant type {other}"))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `DeviceParams` line exactly as ChiXie3 3.0.310 wrote it (device "guangku").
    const INI: &str = r"[DeviceConfig]
AutoConnect=false
DeviceParams=@Variant(\0\0\0\b\0\0\0\x1\0\0\0\xe\0g\0u\0\x61\0n\0g\0k\0u\0\0\0\b\0\0\0\n\0\0\0\x10\0y\0_\0r\0\x65\0\x64\0u\0\x63\0\x65\0\0\0\n\0\0\0\x4\0Y\0-\0\0\0\f\0y\0_\0p\0l\0u\0s\0\0\0\n\0\0\0\x4\0Y\0+\0\0\0\x10\0x\0_\0r\0\x65\0\x64\0u\0\x63\0\x65\0\0\0\n\0\0\0\x4\0X\0-\0\0\0\f\0x\0_\0p\0l\0u\0s\0\0\0\n\0\0\0\x4\0X\0+\0\0\0\x14\0w\0r\0i\0t\0\x65\0S\0p\0\x65\0\x65\0\x64\0\0\0\n\0\0\0\n\0\x31\0\x39\0\x30\0\x30\0\x30\0\0\0\n\0p\0\x65\0n\0U\0p\0\0\0\x2\0\0\0\0\0\0\0\xe\0p\0\x65\0n\0\x44\0o\0w\0n\0\0\0\n\0\0\0\x2\0\x32\0\0\0\x12\0p\0\x61\0u\0s\0\x65\0T\0i\0m\0\x65\0\0\0\n\0\0\0\x2\0\x30\0\0\0\x10\0\x66\0i\0r\0m\0w\0\x61\0r\0\x65\0\0\0\x2\0\0\0\x3\0\0\0\x14\0\x64\0\x65\0v\0i\0\x63\0\x65\0T\0y\0p\0\x65\0\0\0\x2\0\0\0\0)
";

    #[test]
    fn imports_the_real_device_params() {
        let ds = import(INI).unwrap();
        assert_eq!(ds.len(), 1);
        let d = &ds[0];
        assert_eq!(d.name, "guangku");
        assert_eq!((d.device_type, d.firmware), (DeviceType::StepperPenUp, Firmware::ChiXieEsp32Grbl));
        assert_eq!((d.pen_up_z, d.pen_down_z), (0.0, 2.0));
        assert_eq!((d.write_speed, d.travel_speed, d.pause_after_down_s), (19000.0, 19000.0, 0.0));
        assert!(d.axis_map.is_identity());
        assert_eq!((d.pen_up_cmd(), d.pen_down_cmd()), ("G1G90 Z0".to_string(), "G1G90 Z2".to_string()));
    }

    #[test]
    fn enum_orders_match_chixie3_dropdowns() {
        let f: Vec<Firmware> = (0..4).map(firmware).collect();
        assert_eq!(f, [Firmware::Grbl, Firmware::Marlin, Firmware::Esp32Grbl, Firmware::ChiXieEsp32Grbl]);
        let t: Vec<DeviceType> = (0..6).map(device_type).collect();
        assert_eq!(
            t,
            [
                DeviceType::StepperPenUp,
                DeviceType::ServoPenUp,
                DeviceType::ElectromagnetPenUp,
                DeviceType::StepperPenUp,
                DeviceType::StepperPenUp,
                DeviceType::CustomZ
            ]
        );
    }

    #[test]
    fn unescapes_qt_ini_bytes() {
        // Hex digits right after an escape are escaped too (`\x61\x62g`, never `\x61bg`), as
        // QSettings writes them, so `\x` can read every following hex digit.
        assert_eq!(unescape(r"\0\x1\b\n\f\x61\x62g\\").unwrap(), vec![0, 1, 8, 10, 12, 0x61, 0x62, b'g', b'\\']);
    }

    #[test]
    fn missing_or_broken_params() {
        assert_eq!(import("[DeviceConfig]\nAutoConnect=false\n").unwrap(), vec![]);
        assert!(import("DeviceParams=@Variant(\\0\\0)").is_err());
        assert!(import("DeviceParams=plain").is_err());
    }

    /// `fixtures/device_params.cset`, written by `make_cset_fixture.py`.
    fn cset_fixture() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/device_params.cset")).unwrap()
    }

    #[test]
    fn imports_both_cset_devices() {
        let ds = import_cset(&cset_fixture()).unwrap();
        assert_eq!(ds.len(), 2);

        let stepper = &ds[0];
        assert_eq!(stepper.name, "测试步进机");
        assert_eq!((stepper.firmware, stepper.device_type), (Firmware::ChiXieEsp32Grbl, DeviceType::StepperPenUp));
        assert_eq!((stepper.pen_up_z, stepper.pen_down_z), (0.0, 2.0));
        assert_eq!((stepper.write_speed, stepper.travel_speed, stepper.pause_after_down_s), (19000.0, 19000.0, 0.5));
        assert_eq!(stepper.end_position, EndPosition::OriginLeft { mm: 20.0 });
        assert!(stepper.axis_map.is_identity());

        let servo = &ds[1];
        assert_eq!(servo.name, "测试舵机");
        assert_eq!((servo.firmware, servo.device_type), (Firmware::Grbl, DeviceType::ServoPenUp));
        assert_eq!((servo.pen_up_z, servo.pen_down_z), (90.0, 30.0));
        assert_eq!((servo.write_speed, servo.travel_speed, servo.pause_after_down_s), (8000.0, 8000.0, 0.2));
        assert_eq!(servo.end_position, EndPosition::Stay);
        // x_plus is Y-, and y_plus is absent: it comes from y_reduce X- as X+.
        assert_eq!(servo.axis_map, AxisMap { swap_xy: true, invert_x: false, invert_y: true });
    }

    #[test]
    fn write_done_pos_maps_to_end_position() {
        let at = |v: f32| {
            let p = Value::Map(vec![("WriteDonePos".into(), Value::Double(f64::from(v)))]);
            profile("d", &p).end_position
        };
        assert_eq!((at(0.0), at(1.0), at(2.0), at(7.0)), (EndPosition::Origin, EndPosition::OriginLeft { mm: 20.0 }, EndPosition::Stay, EndPosition::Origin));
    }

    #[test]
    fn cset_must_be_a_dict_of_dicts() {
        // [1, 2]: not a device table.
        assert!(import_cset(b"\x80\x04]q\x00(K\x01K\x02e.").is_err());
        // {"evil": 1}: a device whose parameters are not a dict.
        assert!(import_cset(b"\x80\x04}(X\x04\x00\x00\x00evilK\x01u.").is_err());
    }

    #[test]
    fn malicious_cset_is_parsed_as_data() {
        // {"evil": os.system("echo pwned")} as GLOBAL + REDUCE: the call is recorded, never made.
        let bytes = b"\x80\x02}(X\x04\x00\x00\x00evilcos\nsystem\n\x8c\x0aecho pwned\x85Ru.";
        let v = crate::pickle::parse(bytes).unwrap();
        assert!(matches!(v.get("evil"), Some(crate::pickle::Value::Object { class, .. }) if class == "os.system"));
        assert!(import_cset(bytes).is_err());
    }
}
