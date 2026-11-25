**English** | [简体中文](README.zh-CN.md)

# chixie-rs

A cross-platform desktop and CLI client for **ChiXie** pen-plotter handwriting machines (Grbl_ESP32 boards), rewritten from scratch in **Rust** to replace the official **ChiXie3** Windows client (Python + PyQt5 + Nuitka).

Type or import a document, lay it out on ruled, composition-grid, or table templates with single-line handwriting fonts, and stream deterministic G-code to your plotter—or export vector PDFs for printing.

> Personal tool running natively on **Windows, macOS, and Linux**. Fonts downloaded with your ChiXie account are saved locally as standard SVG fonts and work **fully offline with or without logging in**.

---

## Advantages Over Original ChiXie3

| Area                                | Original ChiXie3                                                                                                                            | chixie-rs                                                                                                                                                                                                                                                                                 |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Cross-platform & No Vendor Lock** | Windows-only; hides third-party devices on macOS and warns on uncertified boards                                                            | Native **Windows, macOS, and Linux** builds; works with any standard Grbl / Grbl_ESP32 board and Bluetooth SPP serial ports without vendor locks                                                                                                                                          |
| **Deterministic Handwriting**       | Samples randomness at layout time without saving it; reopening `.cdoc` or re-pasting shifts strokes                                         | Every character has a **stable `id`** keyed with a persisted document seed (SplitMix64); editing never reshuffles other characters, and identical docs produce **byte-identical G-code**                                                                                                  |
| **Offline-First**                   | Requires login to type, import, or use file menus; revokes fonts on heartbeat failure; loads tools via web pages                            | **Local-first** font and template libraries; once downloaded or imported, editing, layout, export, and plotting work 100% offline                                                                                                                                                         |
| **Native Performance**              | Per-character `QGraphicsItem` scene (requires "large text mode") and slow Python TPS warping                                                | Flat document model + viewport culling + cached page paths + **deformed glyph cache (`DeformCache`)**; microsecond TPS solves; 16 styled pages relayout in ~**40 ms** and paint in **3–5 ms/frame** (CI debug build)                                                                      |
| **Streaming & Exact Resume**        | Long Telnet jobs stall; CH340 USB drops change COM numbers and break jobs                                                                   | Single device actor, character-counting streamer, `?` polling & `Bf:` resync, VID/PID auto-reconnect on USB drop, **G-code line ↔ `(page, char id, stroke)` map** for **line-exact resume**, and immediate Feed Hold (`!`) pause                                                          |
| **Security & Open Formats**         | Plaintext passwords in `config.ini`; loads `.cx`/`.cset` via Python `pickle` (arbitrary code execution); hard-coded encryption keys         | Passwords stored solely in the OS credential store (`keyring`); **safe opcode-only pickle reader** (never executes code); open formats: **DOCX** (`customXml` preserves seed & char IDs; opens in Word/WPS/LibreOffice), **SVG fonts**, **SVG templates** (+ **XLSX**), **TOML**, **ZIP** |
| **Editor & Typesetting Quality**    | `Ctrl+Z` shows "not implemented yet"; no automatic kinsoku (warnings only); table overflow blocks editing; unfinished arrow/cell navigation | Full command-pattern **Undo/Redo**; real **automatic kinsoku** (with hanging punctuation in composition grids); non-blocking cell overflow policies (shrink / add rows / clip); full Word-style & `Tab` table navigation                                                                  |
| **Self-Contained Documents**        | Templates, origin, and rotation live in global app settings                                                                                 | Template, writing origin, page rotation, and per-char tweaks travel with the document for reproducible output across machines                                                                                                                                                             |

---

## Extra Features Beyond ChiXie3

Beyond parity with the ChiXie3 writing workflow, `chixie-rs` adds:

### 1. Handwriting & Font Enhancements

- **Per-Script Styles (中英文 / 数字分开设置):** Independent size scaling, baseline offset, vertical float, and character deformation for English letters and digits separate from Chinese characters (ChiXie3 only allows picking separate fonts).
- **Cursive Stroke Joining (连笔):** Automatically bridges nearby stroke endpoints inside a character with short pen-down curves to emulate natural fast handwriting.
- **Per-Character Pen Pressure & Jitter (字符轻重与随机轻重):** Five per-character pressure tiers (`更轻Ⅱ` to `更重Ⅱ`) plus configurable per-character random pressure variation (`press_jitter`).
- **Independent Slant & Tail Tuning:** Splits glyph slant (`slant_degree`) from shape warp into separate controls and exposes tail-removal probability and max length thresholds.
- **Watched Single-Line SVG Font Library:** Fonts live as plain SVG files (compatible with Inkscape Hershey Text) in user-accessible folders monitored in real time (`notify`), supporting extra shared/read-only folders and single/ZIP import & export.

### 2. Editing & Typesetting Productivity

- **Real Kinsoku & Grid Word Segmentation:** Automatic line-start/end punctuation rules plus optional `jieba-rs` word segmentation in composition grids (`作文格分词`) to keep multi-character words on the same row.
- **Smart Whitespace Cleanup:** One-click removal of spaces between CJK and Latin/digits or between punctuation and text.
- **Navigation Pane (导航窗格):** Integrated document search results and missing-glyph (`缺字`) jump lists; clicking any row scrolls to and selects the character on the page.
- **Non-Modal Find & Replace:** Word-style top bar supporting case matching and full-width/half-width equivalence while preserving untouched character IDs on replace.
- **Format Painter & Floating Mini Toolbar:** Single-click apply or double-click sticky mode, plus quick inline formatting beside selections.
- **Draft Autosave & Vector PDF Export:** Continuous autosave to `draft.docx` (with legacy `draft.chx` fallback) and vector PDF generation with independent toggles for handwriting ink and template rulings.

### 3. Templates & Local Table Detection

- **Local Table-Frame Detector (`chixie-rs-import`):** Detects tables, merged cells, and cell paddings locally from **vector PDFs** (parsing line/rectangle drawing operators), **scans, or phone photos** (adaptive thresholding, perspective correction, and line run aggregation) with no cloud upload.
- **SVG Template Library & Excel Round-Trip:** Templates are stored as true-scale millimetre SVGs (with `<metadata>` TOML and embedded backgrounds), supporting ZIP bundles, bidirectional `.xlsx` import/export for tables, and `.cx` import from ChiXie3.

### 4. Calibration, Device Extras & CLI

- **Pen-Down Depth Calibration Wizard (落笔校准):** Plots a test row at varying Z depths so you can visually pick the cleanest contact depth and write it straight to the device profile.
- **Firmware & Console Tooling:** Tabular Grbl `$$` parameter editor (with descriptions and typed confirmation for `$RST=*`), custom multi-line command buttons, persistent console history, and safe `config.ini` / `.cset` import.
- **Full-Featured CLI (`chixie-rs-cli`):** Headless G-code generation, PDF rendering, serial/Telnet streaming, ESP3D SD card management, table detection, and a `fake-grbl` capture server.

---

## Core Feature Overview

- **Rich-Text Editor:** Stable character-ID text model, undo/redo, per-run font/size/color/underline/strikethrough/tilt/superscript/subscript/heading, paragraph alignment, first-line indent, single/double-page preview, guide/line-number/page-number toggles, DOCX open/save, TXT (including GB18030) import, PDF export & printing.
- **Templates:** Ruled (`信纸`, including dual-line and multi-column), composition grids (`作文格`), and table templates (`表格`, cell merge/split, interactive row/column resizing, cell data sources, mail-merge batch pages); interactive writing-origin placement.
- **Handwriting & Fonts:** Cloud font download & offline store, weighted multi-font mixing (`换一组`), role slots (`主线`/`英文`/`符号`/`数字`), automatic missing-glyph fallback, glyph size unification/deduplication/width fixes, TPS shape deformation, per-character nudge & tweak panel (`字符微调`).
- **Output & Device Control:** Serpentine (`弓字型`) / Z-order (`之字型`) path planning, 180° odd/even/all page rotation, multi-color pen-change prompts, path simplification/smoothing (`书写优化`), workspace boundary tracing (`巡边`), frame-line plotting, experimental stroke-tail lift (`笔锋测试`), USB auto-connect, jogging, non-modal sheet-change prompts, TF card upload & run, USB-drop recovery, and resume from any clicked character.

---

## Tech Stack

| Layer                          | Technologies & Crates                                                                                                                                                                 |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Language & Workspace**       | **Rust** (Edition 2024, MSRV `1.98`), 8-crate Cargo workspace                                                                                                                         |
| **Desktop GUI**                | **`gpui-kit` (`gpui` / `gpui-pre 0.3.11` + `gpui-component 0.9.0`)**, GPU-accelerated rendering (DX11/Vulkan on Windows, Metal on macOS, Vulkan/Wayland/X11 on Linux)                 |
| **Documents & Office Formats** | Custom lightweight OOXML engine (`zip` + `quick-xml` + `encoding_rs`) for DOCX/TXT; `calamine` & `rust_xlsxwriter` for Excel; `lopdf` for vector PDF parsing and export               |
| **CJK Typesetting**            | Custom layout & kinsoku engine; optional `jieba-rs` (`segment` feature) for composition-grid word segmentation                                                                        |
| **Graphics & File Watching**   | `image` (PNG/JPEG previews & raster table detection); `notify` for live library folder watching; `embed-resource` for Windows icons                                                   |
| **Crypto & Networking**        | `reqwest` (`rustls`) for backend APIs; `aes`, `cbc`, `hmac`, `sha2`, `base64` for font decryption; custom opcode-only pickle reader for `.cx`/`.cset`                                 |
| **Device & Concurrency**       | `serialport` (CH340 / GD32 / Bluetooth SPP), TCP/Telnet & ESP3D HTTP; `async-channel` driving a single-actor device loop                                                              |
| **Config & OS Integration**    | `toml`, `serde`, `serde_json`; `directories` for OS-standard paths; `keyring` for OS credential stores                                                                                |
| **CLI & Quality**              | `clap` (derive), `proptest` property tests, golden G-code regression suites, `CHIXIE_RS_FAKE=1` offline simulator, `CHIXIE_RS_AUTOTEST=1` GUI autotest, native 3-OS GitHub Actions CI |

---

## File Locations

### 1. Repository Layout

```text
chixie-rs/
├── Cargo.toml                     # Workspace members, shared dependencies, and build profiles
├── crates/
│   ├── chixie-rs-core/            # Core: backend API, session, credentials, SVG font library, mixing/fixes, layout, handwriting/cursive/deformation, Settings, ChiXie3 config/.cset import
│   ├── chixie-rs-doc/             # Document model: stable char IDs, attribute runs, paragraphs, undoable commands, DOCX/TXT/.chx I/O, find/replace
│   ├── chixie-rs-template/        # Templates: ruled/grid/table models, cell data sources, SVG library, ZIP/XLSX round-trip, .cx import
│   ├── chixie-rs-import/          # Local table detector: vector PDF (lopdf) and raster/photo grid & merged-cell detection
│   ├── chixie-rs-output/          # Output pipeline: path planning, page rotation/origin transforms, pen-color segments, G-code emitter, vector PDF export
│   ├── chixie-rs-device/          # Device layer: serial/Telnet/ESP3D transports, Grbl streaming & resync, reconnect actor, LAN discovery
│   ├── chixie-rs-gui/             # Desktop app: gpui UI, Ribbon/Backstage, editor, template editor, device drawer, calibration, fake backend/sim, OS resources (resources/)
│   └── chixie-rs-cli/             # CLI binary (chixie-rs-cli) and fake-grbl TCP capture server
└── docs/                          # Specs & notes: chixie3-features.md, reimplementation-plan.md, protocol.md, server-api.md, hardware-checklist.md
```

### 2. Runtime Data & Config Locations

All runtime files follow native OS conventions via `directories`:

| Data                                                 | Windows                                        | macOS                                                   | Linux                                                     | Notes                                                                               |
| ---------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| **Settings** (`settings.toml`)                       | `%APPDATA%\chixie-rs\config\settings.toml`     | `~/Library/Application Support/chixie-rs/settings.toml` | `~/.config/chixie-rs/settings.toml` (`$XDG_CONFIG_HOME`)  | Device profile, page defaults, handwriting style, UI preferences (**no passwords**) |
| **Autosaved Draft** (`draft.docx`)                   | Same directory as `settings.toml`              | Same directory as `settings.toml`                       | Same directory as `settings.toml`                         | Autosaved on edits; falls back to legacy `draft.chx` if present                     |
| **Runtime Caches** (`previews/`, `template_assets/`) | Same directory as `settings.toml`              | Same directory as `settings.toml`                       | Same directory as `settings.toml`                         | Cloud font preview PNGs and active template background PNGs                         |
| **Font Library** (default)                           | `%USERPROFILE%\Documents\chixie-rs\Fonts\`     | `~/Documents/chixie-rs/Fonts/`                          | `~/Documents/chixie-rs/Fonts/` (`$XDG_DOCUMENTS_DIR`)     | One `.svg` per font; movable in **选项 › 字体** with extra watched folders          |
| **Template Library** (default)                       | `%USERPROFILE%\Documents\chixie-rs\Templates\` | `~/Documents/chixie-rs/Templates/`                      | `~/Documents/chixie-rs/Templates/` (`$XDG_DOCUMENTS_DIR`) | One `.svg` per template; movable in **选项 › 模板** with extra read-only folders    |
| **Account Password**                                 | Windows Credential Manager                     | macOS Keychain                                          | Linux Secret Service (D-Bus)                              | Stored strictly in the OS credential store under service `chixie-rs`                |
| **Fake / Demo Mode Data**                            | `%TEMP%\chixie-rs-demo\`                       | `$TMPDIR/chixie-rs-demo/`                               | `/tmp/chixie-rs-demo/`                                    | Isolated when `CHIXIE_RS_FAKE=1` is set; real settings and libraries are untouched  |
| **ChiXie3 Import Sources**                           | `%USERPROFILE%\ChiXie3\conf\`                  | —                                                       | —                                                         | Optional manual import for `config.ini`, `.cx` templates, and `.cset` profiles      |

> Note: Legacy `fonts/` stores and `templates.toml` inside the config directory are automatically migrated to `Fonts/` and `Templates/` on first launch (keeping `templates.toml.bak`).

---

## Hardware

Tested target: a Grbl_ESP32 ChiXie board with a CH340 USB-serial chip (~300 × 300 mm travel), connected by cable. The CH340 can drop off USB under motor load and return on a new port number; leave the port set to `自动` (auto) so the client finds it automatically.

---

## Download

Each CI run on GitHub Actions builds natively on `windows-latest`, `macos-latest`, and `ubuntu-latest` and uploads `chixie-rs-<OS>-debug` artifacts (`chixie-rs` GUI and `chixie-rs-cli`):

- **Windows:** `chixie-rs.exe` and `chixie-rs-cli.exe`; run directly.
- **macOS:** `chixie-rs-macOS.tar.gz` containing `chixie-rs.app`. Unpack and move it to Applications. The app is not notarized, so the first start requires right-click › Open (or `xattr -dr com.apple.quarantine`).
- **Linux:** `chixie-rs-Linux.tar.gz`. Unpack and run `./chixie-rs/install.sh` to install into `~/.local` with a desktop menu entry and icon (`PREFIX=/usr/local` system-wide).

The app icon is generated from [docs/OriginIcon.png](docs/OriginIcon.png) by [make_icons.py](crates/chixie-rs-gui/resources/icon/make_icons.py); generated assets are committed.

---

## Build from Source

Requires **Rust 1.98** or newer (edition 2024).

```sh
cargo build --workspace
cargo run -p chixie-rs-gui
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

Release build：

```sh
cargo build --workspace --release
cargo run -p chixie-rs-gui --release
```

---

## Try It Without a Machine or Account

```sh
CHIXIE_RS_FAKE=1 cargo run -p chixie-rs-gui
```

`CHIXIE_RS_FAKE=1` uses an offline demo backend (any account, password `demo`) and an in-process simulated Grbl board. Settings and libraries go to a temporary directory, leaving your real config and credentials untouched.

Environment variables:

| Variable                                  | Effect                                                                          |
| ----------------------------------------- | ------------------------------------------------------------------------------- |
| `CHIXIE_RS_FAKE=1`                        | Offline demo backend + simulated Grbl device in a temp folder                   |
| `CHIXIE_RS_AUTOTEST=1`                    | Scripted GUI test against the demo backend; prints `AUTOTEST_*` lines and quits |
| `CHIXIE_RS_SIM_DELAY_MS`                  | Per-line delay (ms) for the simulated Grbl device (default `3`)                 |
| `CHIXIE_RS_PERF=1`                        | Logs layout and paint timings                                                   |
| `CHIXIE_RS_ACCOUNT`, `CHIXIE_RS_PASSWORD` | Login credentials for CLI font commands                                         |

---

## Command Line (`chixie-rs-cli`)

Run `chixie-rs-cli --help` to list every command and option:

| Command                                  | Does                                                                                    |
| ---------------------------------------- | --------------------------------------------------------------------------------------- |
| `ports`, `discover`                      | List serial ports or discover wireless boards on the LAN                                |
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

- [docs/chixie3-features.md](docs/chixie3-features.md): ChiXie3 feature breakdown, parity status, and improvements
- [docs/reimplementation-plan.md](docs/reimplementation-plan.md): Architecture and milestones
- [docs/protocol.md](docs/protocol.md): Device protocol and G-code captures
- [docs/server-api.md](docs/server-api.md): Backend API reference
- [docs/hardware-checklist.md](docs/hardware-checklist.md): Hardware verification checklist
