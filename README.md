**English** | [简体中文](README.zh-CN.md)

# PenKaku

*Formerly `chixie-rs` — see [Rename Proposal](docs/rename-proposal.md).*

An open-source, cross-platform **pen-plotter & handwriting-machine desktop host and CLI** written in **Rust**, targeting DIY and commercial writing plotters built around **CH340 / GD32** USB-serial chips and **GRBL / Grbl_ESP32** controllers (including ChiXie hardware and compatible ecosystems).

Unlike traditional G-code senders built for CNC milling or raw vector art, this project unifies the entire handwriting workflow: **rich-text document editing → ruled/grid/table paper layout → deterministic handwriting synthesis → resilient serial streaming and line-exact resume**—plus direct vector PDF export for printing.

> Runs natively on **Windows, macOS, and Linux**. Uses standard single-line SVG fonts (compatible with Inkscape Hershey Text) and works **100% offline**, while keeping backward compatibility with ChiXie cloud fonts and ChiXie3 import formats.
>
> **ChiXie account login is supported:** sign in with your existing ChiXie (炽写) account to browse the cloud font catalog and download your fonts into the local library. Login is optional — once downloaded, fonts stay on disk and everything works offline.

---

## Position in the Open-Source Plotter Ecosystem

In the open-source **CH340 + Arduino/ESP32 + GRBL** plotter ecosystem (such as `grbl/grbl`, `cprezzi/grbl-servo`, `bdring/Grbl_Esp32`, `bdring/FluidNC`, `bdring/midTbot_esp32`, `bdring/TMC2209_Pen_Laser`, and DIY CNC Shield V3 / CoreXY / H-Bot plotters), existing open-source host tools (**GRBL-Plotter**, **Universal G-Code Sender**, **LaserGRBL**, **Candle**, or browser WebSerial senders) focus on generic SVG/DXF/CNC workflows and lack document typesetting, CJK ruled/grid/table layouts, and realistic handwriting synthesis. Meanwhile, proprietary vendor clients (such as ChiXie3) are typically Windows-only, vendor-locked, and cloud-dependent.

`PenKaku` aims to be a **modern, open-source Rust host for CH340 / GRBL handwriting plotters**:

### 1. Compared with Open-Source GRBL Senders (GRBL-Plotter, UGS, LaserGRBL, etc.)
- **Document-First Workflow:** No need to convert text to curves in Inkscape before exporting SVG/G-code. Edit rich text directly with Word-style formatting and lay it out onto ruled paper, dual-line/multi-column sheets, composition grids, or complex merged-cell tables.
- **CJK Typesetting & Realistic Handwriting:** Built-in automatic kinsoku, grid word segmentation, `SplitMix64` deterministic jitter, TPS glyph deformation, cursive stroke joining, and per-character pressure variation.
- **Native Rust + GPU Rendering:** Zero JVM, .NET, or Electron/WebEngine overhead—a single native binary across Windows, macOS, and Linux.

### 2. Advantages Over Traditional Proprietary Clients (e.g., ChiXie3)

| Area                                | Proprietary Client (ChiXie3)                                                                                | PenKaku                                                                                                                                                                                                                          |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Cross-Platform & No Vendor Lock** | Windows-only; hides third-party devices on macOS and warns on uncertified boards                            | Native **Windows, macOS, and Linux**; works with generic Grbl / Grbl_ESP32 / Bluetooth SPP serial ports and stepper, servo, solenoid, or custom Z pen lifts                                                                      |
| **Deterministic Handwriting**       | Samples randomness at layout time without saving it; reopening or re-pasting shifts strokes                 | Every character carries a **stable `id`** keyed with a persisted seed; editing never reshuffles other characters, and identical docs yield **byte-identical G-code**                                                             |
| **True Offline-First**              | Requires login to type, import, or use file menus; revokes fonts on heartbeat failure                       | **Local-first** SVG font and template libraries; editing, layout, export, and plotting work 100% offline                                                                                                                         |
| **Native Performance**              | Per-character `QGraphicsItem` scene (requires "large text mode") and slow Python TPS warping                | Flat model + viewport culling + cached page paths + **deformed glyph cache (`DeformCache`)**; microsecond TPS solves; 16 styled pages relayout in ~**40 ms** and paint in **3–5 ms/frame** (CI debug build)                      |
| **CH340 Recovery & Exact Resume**   | CH340 drops under motor load and switches COM numbers, breaking jobs                                        | Single device actor, character-counting streamer, `?` polling & `Bf:` resync, auto-reconnect on CH340/GD32 drop, **G-code line ↔ `(page, char id, stroke)` map** for **line-exact resume**, and immediate Feed Hold (`!`) pause  |
| **Security & Open Formats**         | Plaintext passwords in `config.ini`; Python `pickle` deserialization (code execution risk); hard-coded keys | Passwords stored solely in the OS credential store (`keyring`); **safe opcode-only pickle reader**; open formats: **DOCX** (`customXml` keeps seed & char IDs), **SVG fonts**, **SVG templates** (+ **XLSX**), **TOML**, **ZIP** |
| **Editor & Typesetting Parity+**    | `Ctrl+Z` unimplemented; warning-only kinsoku; modal table overflow blocking                                 | Full command-pattern **Undo/Redo**; real **automatic kinsoku** (with hanging punctuation in grids); non-blocking overflow policies (shrink / add rows / clip); full Word-style & `Tab` table navigation                          |
| **Self-Contained Documents**        | Templates, origin, and rotation live in global settings                                                     | Template, writing origin, page rotation, and per-char tweaks stay inside the document for reproducible output                                                                                                                    |

---

## Extra Features Beyond Traditional Clients

- **Per-Script Styles (中英文 / 数字分开设置):** Independent size scaling, baseline offset, vertical float, and deformation for English letters and digits separate from Chinese characters.
- **Cursive Stroke Joining (连笔):** Bridges close stroke endpoints within a glyph using short pen-down curves for natural fast-writing flow.
- **Per-Character Pen Pressure & Jitter (字符轻重与随机轻重):** Five per-character Z-depth pressure tiers (`更轻Ⅱ` to `更重Ⅱ`) plus configurable random pressure variation (`press_jitter`).
- **Independent Slant & Tail Controls:** Separates glyph slant (`slant_degree`) from shape warp and exposes tail-removal probability and max length settings.
- **Pen-Down Calibration Wizard (落笔校准):** Plots a test row across Z depths so you can visually pick the cleanest contact depth and apply it directly to your device profile.
- **Local Table-Frame Detector (`penkaku-import`):** Detects grids, merged cells, and cell paddings locally from **vector PDFs** (line/rect operators), **scans, or phone photos** with no cloud dependency.
- **Watched SVG Font & Template Libraries:** Plain single-line SVG fonts (Inkscape Hershey Text compatible) and true-scale mm SVG templates stored in watched folders (`notify`) with support for extra shared/read-only folders and ZIP/XLSX import & export.
- **Kinsoku, Grid Segmentation & Space Cleanup:** Automatic line-start/end punctuation rules, optional `jieba-rs` word segmentation in composition grids, and one-click cleanup of CJK–Latin or punctuation spacing.
- **Navigation Pane & Non-Modal Find/Replace:** Sidebar jump lists for search matches and missing glyphs (`缺字`), plus case-sensitive and full-width/half-width-equivalent find/replace that preserves untouched character IDs.
- **Format Painter, Mini Toolbar, Autosave & Vector PDF:** Sticky format painter, floating selection bar, `draft.docx` autosave, and vector PDF export with independent ink/background toggles.
- **ChiXie Account Login & Cloud Fonts (炽写账号登录):** Log in with a ChiXie account in the GUI (or `PENKAKU_ACCOUNT` / `PENKAKU_PASSWORD` for the CLI) to list the online font catalog and your collected fonts, then download them as local SVG fonts. "Remember me" keeps the password only in the OS credential store.
- **Full CLI (`penkaku-cli`):** Headless G-code generation, PDF export, serial/Telnet streaming, ESP3D TF card management, table detection, and a `fake-grbl` capture server.

---

## Supported Hardware & Firmware

| Category               | Support                                                                                   | Notes                                                                                                                                       |
| ---------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| **USB-Serial Chips**   | **CH340**, **GD32 Virtual Com Port**, generic USB-UART & Bluetooth SPP                    | Auto-mode prioritizes `USB-SERIAL CH340` / `GD32 Virtual Com Port`; automatically scans and recovers after USB drops                        |
| **Firmware**           | **GRBL** (`grbl` / `grbl-servo`), **ESP32_Grbl / Grbl_ESP32** (`ChiXie_ESP32_Grbl`, etc.) | USB serial (`115200` baud default), Wi-Fi Telnet (port `23`), and ESP3D HTTP (TF card upload); Marlin streaming not yet enabled             |
| **Pen-Lift Actuators** | **Stepper**, **RC Servo**, **Solenoid / Electromagnet**, **Custom Z G-code**              | Separate write/travel feeds, configurable pen-down dwell (`G4 P`), axis swap/invert (`AxisMap`), experimental stroke-tail lift (`笔锋测试`) |
| **Verified Reference** | CH340 + Grbl_ESP32 1.3a board (~300 × 300 mm travel)                                      | Keep port on `自动` (auto) so the client finds the new port number if CH340 drops under motor load                                          |

---

## Tech Stack

| Layer                          | Technologies & Crates                                                                                                                                                                   |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Language & Workspace**       | **Rust** (Edition 2024, MSRV `1.98`), 8-crate Cargo workspace                                                                                                                           |
| **Desktop GUI**                | **`gpui-kit` (`gpui` / `gpui-pre 0.3.11` + `gpui-component 0.9.0`)**, GPU-accelerated rendering (DX11/Vulkan on Windows, Metal on macOS, Vulkan/Wayland/X11 on Linux)                   |
| **Documents & Office Formats** | Custom lightweight OOXML engine (`zip` + `quick-xml` + `encoding_rs`) for DOCX/TXT (incl. GB18030); `calamine` & `rust_xlsxwriter` for Excel; `lopdf` for vector PDF parsing and export |
| **CJK Typesetting**            | Custom layout & kinsoku engine; optional `jieba-rs` (`segment` feature) for composition-grid word segmentation                                                                          |
| **Graphics & File Watching**   | `image` (PNG/JPEG previews & raster table detection); `notify` for live library folder watching; `embed-resource` for Windows icons                                                     |
| **Device & Concurrency**       | `serialport` (CH340 / GD32 / Bluetooth SPP), TCP/Telnet & ESP3D HTTP; `async-channel` driving a single-actor device loop                                                                |
| **Crypto & Legacy Import**     | `reqwest` (`rustls`) for optional cloud font catalog; `aes`, `cbc`, `hmac`, `sha2`, `base64`; custom opcode-only pickle reader for `.cx`/`.cset`                                        |
| **Config & OS Integration**    | `toml`, `serde`, `serde_json`; `directories` for OS-standard paths; `keyring` for OS credential stores                                                                                  |
| **CLI & Quality**              | `clap` (derive), `proptest` property tests, golden G-code regression suites, `PENKAKU_FAKE=1` offline simulator, `PENKAKU_AUTOTEST=1` GUI autotest, native 3-OS GitHub Actions CI       |

---

## File Locations

### 1. Repository Layout

```text
penkaku/
├── Cargo.toml                     # Workspace members, shared dependencies, and build profiles
├── crates/
│   ├── penkaku-core/            # Core: SVG font library, mixing/fixes, layout, handwriting/cursive/deformation, Settings, optional API & legacy config/.cset import
│   ├── penkaku-doc/             # Document model: stable char IDs, attribute runs, paragraphs, undoable commands, DOCX/TXT/.chx I/O, find/replace
│   ├── penkaku-template/        # Templates: ruled/dual-line/column/grid/table models, cell data sources, SVG library, ZIP/XLSX round-trip, .cx import
│   ├── penkaku-import/          # Local table detector: vector PDF (lopdf) and raster/photo grid & merged-cell detection
│   ├── penkaku-output/          # Output pipeline: Z/serpentine planning, page rotation/origin transforms, pen-color segments, G-code emitter, vector PDF export
│   ├── penkaku-device/          # Device layer: serial/Telnet/ESP3D transports, Grbl streaming & resync, CH340 reconnect actor, LAN discovery
│   ├── penkaku-gui/             # Desktop app: gpui UI, Ribbon/Backstage, editor, template editor, device drawer, calibration, fake backend/sim, OS resources (resources/)
│   └── penkaku-cli/             # CLI binary (penkaku-cli) and fake-grbl TCP capture server
└── docs/                          # Specs, rename proposal, protocol captures, and checklists
```

### 2. Runtime Data & Config Locations

| Data                                                 | Windows                                      | macOS                                                 | Linux                                                   | Notes                                                                               |
| ---------------------------------------------------- | -------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| **Settings** (`settings.toml`)                       | `%APPDATA%\penkaku\config\settings.toml`     | `~/Library/Application Support/penkaku/settings.toml` | `~/.config/penkaku/settings.toml` (`$XDG_CONFIG_HOME`)  | Device profile, page defaults, handwriting style, UI preferences (**no passwords**) |
| **Autosaved Draft** (`draft.docx`)                   | Same directory as `settings.toml`            | Same directory as `settings.toml`                     | Same directory as `settings.toml`                       | Autosaved on edits; falls back to legacy `draft.chx` if present                     |
| **Runtime Caches** (`previews/`, `template_assets/`) | Same directory as `settings.toml`            | Same directory as `settings.toml`                     | Same directory as `settings.toml`                       | Font preview PNGs and active template background PNGs                               |
| **Font Library** (default)                           | `%USERPROFILE%\Documents\penkaku\Fonts\`     | `~/Documents/penkaku/Fonts/`                          | `~/Documents/penkaku/Fonts/` (`$XDG_DOCUMENTS_DIR`)     | One `.svg` per font; movable in **选项 › 字体** with extra watched folders          |
| **Template Library** (default)                       | `%USERPROFILE%\Documents\penkaku\Templates\` | `~/Documents/penkaku/Templates/`                      | `~/Documents/penkaku/Templates/` (`$XDG_DOCUMENTS_DIR`) | One `.svg` per template; movable in **选项 › 模板** with extra read-only folders    |
| **Account Password**                                 | Windows Credential Manager                   | macOS Keychain                                        | Linux Secret Service (D-Bus)                            | Stored strictly in the OS credential store under service `penkaku`                  |
| **Fake / Demo Mode Data**                            | `%TEMP%\penkaku-demo\`                       | `$TMPDIR/penkaku-demo/`                               | `/tmp/penkaku-demo/`                                    | Isolated when `PENKAKU_FAKE=1` is set; real settings and libraries are untouched    |
| **ChiXie3 Import Sources**                           | `%USERPROFILE%\ChiXie3\conf\`                | —                                                     | —                                                       | Optional manual import for `config.ini`, `.cx` templates, and `.cset` profiles      |

---

## Download

Each CI run on GitHub Actions builds natively on `windows-latest`, `macos-latest`, and `ubuntu-latest` and uploads `penkaku-<OS>-debug` artifacts (`penkaku` GUI and `penkaku-cli`):

- **Windows:** `penkaku.exe` and `penkaku-cli.exe`; run directly (install the WCH CH340 driver if your board's COM port does not appear automatically).
- **macOS:** `penkaku-macOS.tar.gz` containing `penkaku.app`. Unpack and move it to Applications. The app is not notarized, so the first start requires right-click › Open (or `xattr -dr com.apple.quarantine`).
- **Linux:** `penkaku-Linux.tar.gz`. Unpack and run `./penkaku/install.sh` to install into `~/.local` with a desktop menu entry and icon (`PREFIX=/usr/local` system-wide). Ensure your user is in the `dialout` group for serial access.

The app icon is generated from [docs/OriginIcon.png](docs/OriginIcon.png) by [make_icons.py](crates/penkaku-gui/resources/icon/make_icons.py); generated assets are committed.

---

## Build from Source

Requires **Rust 1.98** or newer (edition 2024).

```sh
cargo build --workspace
cargo run -p penkaku-gui
```

On Linux (Debian / Ubuntu), install the system dependencies first:

```sh
sudo apt-get install -y pkg-config libudev-dev libdbus-1-dev libfontconfig-dev libfreetype-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
  libvulkan-dev mesa-vulkan-drivers libzstd-dev
```

Run tests and lints:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Try It Without Hardware or an Account

```sh
PENKAKU_FAKE=1 cargo run -p penkaku-gui
```

`PENKAKU_FAKE=1` uses an offline demo backend and an in-process simulated Grbl board. Settings and libraries go to a temporary directory, leaving your real config and credentials untouched.

Environment variables:

| Variable                              | Effect                                                                          |
| ------------------------------------- | ------------------------------------------------------------------------------- |
| `PENKAKU_FAKE=1`                      | Offline demo backend + simulated Grbl device in a temp folder                   |
| `PENKAKU_AUTOTEST=1`                  | Scripted GUI test against the demo backend; prints `AUTOTEST_*` lines and quits |
| `PENKAKU_SIM_DELAY_MS`                | Per-line delay (ms) for the simulated Grbl device (default `3`)                 |
| `PENKAKU_PERF=1`                      | Logs layout and paint timings                                                   |
| `PENKAKU_ACCOUNT`, `PENKAKU_PASSWORD` | Login credentials for CLI font commands                                         |

---

## Command Line (`penkaku-cli`)

Run `penkaku-cli --help` to list every command and option:

| Command                                  | Does                                                                                    |
| ---------------------------------------- | --------------------------------------------------------------------------------------- |
| `ports`, `discover`                      | List serial ports (highlighting CH340/GD32) or discover wireless boards on the LAN      |
| `send`, `stream`                         | Send one command or stream a G-code file over serial or `--ip` Telnet                   |
| `sd`                                     | Manage TF card files over ESP3D HTTP (`ls` / `upload` / `rm` / `run` / `status`)        |
| `fonts`, `font-check`                    | List local/collected fonts, browse online catalog, or diagnose font decryption          |
| `write`                                  | Lay out text into G-code, print to stdout/file, or stream directly to the board         |
| `pdf`                                    | Lay out text on a ruled A4 page and save as vector PDF (`--no-ink` / `--no-background`) |
| `detect-table`                           | Detect table frames and merged cells locally from a PDF or image (PNG/JPEG)             |
| `style-template`, `import-style`         | Write a TOML style profile or convert ChiXie3's `config.ini`                            |
| `calibrate`, `capture-diff`, `fake-grbl` | Fit font scale, compare G-code captures, or run a fake Grbl TCP capture server          |

---

## Documentation

- [docs/rename-proposal.md](docs/rename-proposal.md): Why the project was renamed to PenKaku, and the rename checklist
- [docs/chixie3-features.md](docs/chixie3-features.md): ChiXie3 compatibility breakdown and feature matrix
- [docs/reimplementation-plan.md](docs/reimplementation-plan.md): Architecture and milestones
- [docs/protocol.md](docs/protocol.md): Device protocol and G-code captures
- [docs/hardware-checklist.md](docs/hardware-checklist.md): Hardware verification checklist
