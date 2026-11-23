**English** | [简体中文](README.zh-CN.md)

# chixie-rs

A desktop client for ChiXie pen-plotter handwriting machines (Grbl_ESP32 boards), written in
Rust. It replaces the ChiXie3 Windows client: you type a document, lay it out on a template in
a handwriting font, and the machine writes it with a pen.

Internal tool. Runs on Windows, macOS and Linux. Fonts come from your ChiXie account.

## Features

- **Editor:** rich text with stable character ids, undo, find and replace, formats (underline,
  tilt, superscript and subscript, headings), DOCX open and save, TXT import, PDF export and print.
- **Typesetting:** 信纸 (ruled), 作文格 (composition grid) and table templates, Chinese
  punctuation rules, alignment, indents, page numbers.
- **Templates:** template library with ZIP export and import, Excel round trip for tables,
  table detection from PDFs, scans and phone photos, ChiXie3 `.cx` template import.
- **Handwriting:** mixed fonts with weights, a separate English font, handwriting jitter,
  character deformation, per-character tuning.
- **Per-script styles:** English letters and digits get their own size, baseline, vertical
  float and deformation, separate from Chinese (ChiXie3 cannot do this).
- **Output:** stroke ordering, serpentine paths, page rotation, multi-colour pens with change
  prompts, path simplification, frame lines, edge tracing.
- **Device:** auto-connect over USB serial, jog, pen up and down, set origin, multi-page jobs
  with sheet-change prompts, pause and resume, automatic reconnect and resume after a USB drop,
  resume from a chosen character.
- **Device extras:** Grbl `$$` firmware settings editor, custom command buttons, console
  history, ChiXie3 `config.ini` and `.cset` import.
- **Performance:** long documents stay responsive. On 16 styled pages, relayout takes about
  40 ms and a frame paints in 3 to 5 ms (CI debug build).

## Hardware

Tested target: a Grbl_ESP32 ChiXie board with a CH340 USB-serial chip, about 300 × 300 mm
travel, connected by cable. The CH340 can drop off USB under motor load and come back on a new
port; leave the port on 自动 (auto) so the client finds it again.

## Download

Each CI run on GitHub Actions uploads `chixie-rs-<OS>-debug` builds for Windows, macOS and Linux
(the GUI `chixie-rs` and the command line `chixie-rs-cli`).

## Build from source

Requires Rust 1.98 or newer (edition 2024).

```sh
cargo build --workspace
cargo run -p chixie-rs-gui
```

On Linux, install the system packages first (Debian / Ubuntu):

```sh
sudo apt-get install -y pkg-config libudev-dev libdbus-1-dev libfontconfig-dev libfreetype-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
  libvulkan-dev mesa-vulkan-drivers libzstd-dev
```

Checks:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Try it without a machine or account

```sh
CHIXIE_RS_FAKE=1 cargo run -p chixie-rs-gui
```

`CHIXIE_RS_FAKE=1` uses an offline demo backend (any account, password `demo`) and a simulated
Grbl board. Settings go to a temporary folder, so your real settings and saved password are not
touched.

Other switches:

| Variable | Effect |
|---|---|
| `CHIXIE_RS_AUTOTEST=1` | scripted GUI test against the demo backend; prints `AUTOTEST ...` lines and quits |
| `CHIXIE_RS_PERF=1` | logs layout and paint timings |
| `CHIXIE_RS_ACCOUNT`, `CHIXIE_RS_PASSWORD` | login for the CLI font commands |

## Command line

`chixie-rs-cli --help` lists every command. The main ones:

| Command | Does |
|---|---|
| `ports` | list serial ports |
| `send`, `stream` | send one command or stream a G-code file to the board |
| `fonts` | list your collected fonts or browse the online catalog |
| `write` | turn text into G-code, print it, or stream it to the board |
| `pdf` | lay text out on a ruled A4 page and save it as PDF |
| `style-template`, `import-style` | write a style profile, or convert ChiXie3's `config.ini` into one |

## Workspace

| Crate | Contents |
|---|---|
| `chixie-rs-core` | backend API, fonts, layout, handwriting, settings, pickle reader |
| `chixie-rs-device` | serial and Telnet transports, Grbl streaming, device actor |
| `chixie-rs-doc` | document model, undoable commands, DOCX |
| `chixie-rs-template` | page templates, tables, ChiXie3 `.cx` import |
| `chixie-rs-import` | table detection from PDFs, scans and photos |
| `chixie-rs-output` | stroke order, page transforms, pen colours, G-code |
| `chixie-rs-gui` | desktop app (gpui + gpui-component) |
| `chixie-rs-cli` | command line |

## Documentation

- [docs/chixie3-features.md](docs/chixie3-features.md): ChiXie3 features and their status here
- [docs/reimplementation-plan.md](docs/reimplementation-plan.md): milestones
- [docs/hardware-checklist.md](docs/hardware-checklist.md): checks to run on the real machine
- [docs/protocol.md](docs/protocol.md): device protocol and G-code notes
