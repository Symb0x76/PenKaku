# 项目重命名提案与候选名称（Rename Proposal）

> **已决定（2026-11-26）：** 新名称为 **PenKaku**（*Pen* + 日语“書く”）。Crate 为 `penkaku-*`，图形端二进制 `penkaku`、命令行 `penkaku-cli`，配置目录与凭据服务名 `penkaku`，环境变量 `PENKAKU_*`。旧目录、旧凭据、`CHIXIE_RS_*` 变量与旧 XML 命名空间仍可读取（见 README“从 `chixie-rs` 升级”）。下文为当时的提案原文。

## 1. 为什么需要重命名

`chixie-rs` 最初作为“赤写（ChiXie3）”客户端的 Rust 重写代号。但随着项目演进，当前能力已远超单一品牌复刻，核心价值是成为**面向 CH340 / GRBL / Grbl_ESP32 等通用开源及商用笔式写字机的跨平台 Rust 上位机**：

1. **去品牌绑定：** 摆脱“赤写复刻品”的定位误解，吸引使用 Arduino + CNC Shield、ESP32 FluidNC/Grbl_ESP32、CoreXY/H-Bot 等 DIY 写字机的开源社区用户与贡献者。
2. **突出自身核心差异：** 区别于纯矢量绘图上位机（GRBL-Plotter / UGS / LaserGRBL），新名字应体现**“手写体（Handwriting）+ 排版（Typesetting）+ 落笔绘图（Plotter）”**的独特定位。
3. **跨平台工程规范：** 确定统一的名称后，可将二进制名、Crate 前缀、配置目录、环境变量及 XML 命名空间一次性规范化。

---

## 2. 命名原则

- **简短好记：** 英文名 1–2 个音节（4–10 个字母），方便作为 CLI 命令与仓库名；无需强制带 `-rs` 后缀（或仅在内部识别，对外使用独立品牌名）。
- **语义清晰：** 传达“钢笔/笔尖（Nib / Pen / Quill）”“手写/墨迹（Ink / Script / Hand / Stroke）”“排版/纸张（Sheet / Folio / Grid）”“绘图机（Plot）”等意象。
- **跨平台无歧义：** 全小写连字符（如 `inkplot`、`penflow`），避免与常见系统工具（`write`、`plot`、`pen`）或 crates.io 已有热门包直接冲突。

---

## 3. 候选新名字列表

### 方向 A：强调“笔迹 + 写字机/绘图”（最推荐，直击通用上位机定位）

| 英文名 | 建议中文名 | CLI 命令 | 寓意与特点 | 优缺点 |
|---|---|---|---|---|
| **InkPlot** (`inkplot`) | 墨绘 / 墨写 | `inkplot` / `inkplot-cli` | *Ink*（墨水/笔迹）+ *Plot*（绘图机/写字机），极易理解 | **优点：** 直观、专业、国际化；**缺点：** 需确认 crates.io / GitHub 是否已有同名工具 |
| **NibFlow** (`nibflow`) | 笔韵 / 笔流 | `nibflow` / `nibflow-cli` | *Nib*（钢笔尖）+ *Flow*（行云流水/流式发送 G-code），兼顾书写质感与串口流控 | **优点：** 极少重名，兼顾“笔迹流畅”和“G-code 流”；**缺点：** *Nib* 偏书面，略文艺 |
| **PenGrid** (`pengrid`) | 格笔 / 笔格 | `pengrid` / `pengrid-cli` | *Pen*（写字笔）+ *Grid*（信纸/作文格/表格网格），直指核心优势“文档网格排版” | **优点：** 辨识度极高，精准区别于普通绘图软件；**缺点：** 偏重“表格/格子”印象 |
| **ScriptPlot** (`scriptplot`) | 书绘 | `scriptplot` | *Script*（手写体/脚本）+ *Plot*（落笔绘制） | **优点：** 语义极准；**缺点：** 名字稍长（10 字母），敲命令略长 |

### 方向 B：强调“手写与单线字体”（偏向手写体验与文稿排版）

| 英文名 | 建议中文名 | CLI 命令 | 寓意与特点 | 优缺点 |
|---|---|---|---|---|
| **HandPlot** (`handplot`) | 手绘写 / 拟写 | `handplot` | *Hand*（手写）+ *Plot*（写字机） | **优点：** 一眼看懂“手写绘图机”；**缺点：** 英文直白，略偏功能描述 |
| **Kaku** (`kaku`) | 书 / 墨书 | `kaku` | 日语“書く”（书写）罗马音，短小精悍 | **优点：** 4 字母极短，CLI 手感极佳；**缺点：** 与某终端/其他开源项目可能有重名风险 |
| **StrokeNote** (`strokenote`) | 笔笺 / 迹笺 | `strokenote` | *Stroke*（单线笔画）+ *Note*（信纸/笔记） | **优点：** 贴合“单线字体 + 信纸文档”；**缺点：** 硬件控制感稍弱 |

### 方向 C：突出“开源硬件上位机 / Rust 引擎”

| 英文名 | 建议中文名 | CLI 命令 | 寓意与特点 | 优缺点 |
|---|---|---|---|---|
| **GrblPen** (`grblpen`) | — | `grblpen` | *GRBL* + *Pen*，直接表明是 GRBL 写字机专用上位机 | **优点：** 搜索友好（SEO），GitHub 玩家一看即懂；**缺点：** 未来若拓展 Marlin/Bambu/其他协议略受限 |
| **OxidePen** (`oxidepen`) | 锈笔 | `oxidepen` | *Oxide*（Rust 铁锈意象）+ *Pen* | **优点：** Rust 社区味道浓；**缺点：** 终端用户不易直接联想到写字机 |
| **PlotterRS** (`plotter-rs`) | — | `plotter-rs` | 通用 Rust 绘图机上位机 | **优点：** 清晰直白；**缺点：** 缺乏手写排版差异化记忆点 |

### 综合推荐（Top 3）

1. **首选：`InkPlot`**（或 **`NibFlow`**）—— 兼具“墨迹手写”与“硬件绘图机”双重含义，适合作为开源项目长期品牌。
2. **偏排版差异化：`PenGrid`** —— 突出“信纸/作文格/表格排版写字机”这一与 GRBL-Plotter 等传统上位机拉开差距的核心卖点。
3. **偏极客与搜索流量：`GrblPen`** —— 直接锚定 CH340/GRBL 写字机开源硬件圈，最容易被 DIY 用户搜到。

---

## 4. 确认新名字后的代码库重命名清单（基于当前仓库盘点）

> 以下清单基于当前源码扫描整理；**ChiXie3 格式兼容层**（如 `chixie3_templates.rs`、`chixie3_devices.rs`、`Firmware::ChiXieEsp32Grbl`）属于对外兼容支持，建议继续保留 `chixie3` 命名，其余工程标识按以下范围整体替换。

### 4.1 Workspace 与 Crate 名称
- `Cargo.toml`：workspace `members` 及 `[workspace.dependencies]` 中的 8 个 crate：
  - `chixie-rs-core` → `<new>-core`
  - `chixie-rs-doc` → `<new>-doc`
  - `chixie-rs-template` → `<new>-template`
  - `chixie-rs-import` → `<new>-import`
  - `chixie-rs-output` → `<new>-output`
  - `chixie-rs-device` → `<new>-device`
  - `chixie-rs-gui` → `<new>-gui`
  - `chixie-rs-cli` → `<new>-cli`
- 同步更新各 crate 目录名、内部 `Cargo.toml` 及 Rust 源码 `use chixie_rs_*` 导入路径。

### 4.2 二进制、窗口标题与三端打包资源
- `crates/chixie-rs-gui/Cargo.toml`：`[[bin]] name`
- `crates/chixie-rs-cli/Cargo.toml`：`[[bin]] name`
- Windows：`crates/chixie-rs-gui/chixie-rs.rc`、`build.rs`
- macOS：`crates/chixie-rs-gui/resources/macos/Info.plist`（`CFBundleName`、`CFBundleIdentifier`、`CFBundleExecutable`）
- Linux：`crates/chixie-rs-gui/resources/linux/chixie-rs.desktop`、`install.sh`、X11 `app_id`（`src/main.rs`）
- CI 工作流：`.github/workflows/ci.yml` 中的产物包名与二进制路径

### 4.3 运行时目录、凭据与环境变量
- 配置目录与凭据服务名：
  - `crates/chixie-rs-core/src/config.rs`：`ProjectDirs::from("", "", "<new>")`
  - `crates/chixie-rs-core/src/credentials.rs`：`SERVICE = "<new>"`
  - `crates/chixie-rs-core/src/font/library.rs` & `crates/chixie-rs-template/src/library.rs`：`Documents/<NewName>/{Fonts,Templates}`
  - **迁移建议：** 首次启动时检查旧目录（`chixie-rs`），若存在配置或字体/模板目录则提示或自动迁移，避免老用户数据丢失。
- 环境变量（`crates/chixie-rs-gui/src/main.rs`、`crates/chixie-rs-cli/src/main.rs` 等）：
  - `CHIXIE_RS_FAKE`、`CHIXIE_RS_AUTOTEST`、`CHIXIE_RS_SIM_DELAY_MS`、`CHIXIE_RS_PERF`、`CHIXIE_RS_ACCOUNT`、`CHIXIE_RS_PASSWORD` → 统一改为 `<NEW>_*`（可短期保留旧变量 fallback）。
- 文档与字体自定义命名空间（需注意旧文件兼容读取）：
  - `crates/chixie-rs-doc/src/docx.rs`：`NS = "urn:chixie-rs:doc:1"`（读取时同时兼容旧命名空间）
  - `crates/chixie-rs-core/src/font/svg.rs`：`NS = "urn:chixie-rs:font:1"`（读取时同时兼容旧命名空间）
  - `crates/chixie-rs-template/src/svg.rs`：`<metadata id="chixie-template">`（读取时同时兼容旧标签）
