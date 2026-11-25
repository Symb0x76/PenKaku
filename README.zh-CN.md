[English](README.md) | **简体中文**

# chixie-rs

赤写（ChiXie）笔式手写机（Grbl_ESP32 主板）的跨平台桌面端与命令行客户端，使用 **Rust** 从零重写，用于替代官方 **ChiXie3** Windows 客户端（Python + PyQt5 + Nuitka）。

输入或导入文档，套用信纸/作文格/表格模板与单线手写字体排版预览，即可生成确定性 G-code 驱动写字机书写，或导出矢量 PDF 打印。

> 个人工具，原生支持 **Windows、macOS 和 Linux**。字体通过赤写账号下载一次后，即以标准 SVG 字体保存在本机，之后**无需登录、完全离线**即可使用全部核心功能。

---

## 相比原版 ChiXie3 的核心优势

| 维度                 | 原版 ChiXie3                                                                                                               | chixie-rs                                                                                                                                                                           |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **跨平台与设备兼容** | 仅限 Windows；macOS 屏蔽第三方设备，对未认证主板弹窗警告                                                                   | 原生支持 **Windows / macOS / Linux**，无厂商锁定，支持标准 Grbl / Grbl_ESP32 及蓝牙 SPP 串口                                                                                        |
| **手写确定性**       | 排版时临时抽样且不保存，重开 `.cdoc` 或粘贴会导致字迹与缩放漂移                                                            | 每个字符拥有**稳定 `id`**，结合持久化文档随机种子（SplitMix64），局部编辑不影响其余字符；同文档同字体生成**逐字节一致的 G-code**                                                    |
| **离线可用性**       | 必须登录才能打字、导入或使用文件菜单，心跳失败会吊销字体，部分工具依赖网页                                                 | 字体与模板**本地优先（Local-first）**，下载/导入后完全离线运行，不依赖网页或心跳保活                                                                                                |
| **性能与流畅度**     | 基于 `QGraphicsItem` 逐字建模（需"大文本模式"隐藏视野外图元），Python TPS 变形较慢                                         | 扁平文档模型 + 视口裁剪 + 帧间页面路径缓存 + **形变字形缓存（`DeformCache`）**，单字 TPS 微秒级求解；16 页带手写效果文档重排约 **40 ms**，每帧绘制 **3–5 ms**（CI debug 版）        |
| **串口稳定性与续写** | 长任务 Telnet 易假死卡住；CH340 掉线换口后难以无缝接续                                                                     | 独立设备 Actor + 字符计数流控 + `?` 轮询与 `Bf:` 丢包重同步；CH340 掉线按 VID/PID 自动重连；**G-code 行 ↔ `(页, 字符 id, 笔画)` 映射**支持**行级精确断点续写**；暂停发 `!` 立即停笔 |
| **安全性与隐私**     | `config.ini` **明文保存账号密码**；使用 Python `pickle` 直接反序列化加载 `.cx`/`.cset`（存在代码执行风险）；内置硬编码密钥 | 密码仅存操作系统安全凭据库（`keyring`）；自研**受限 pickle opcode 只读解析器**（绝不执行代码）；不收集或上传云端诊断/缺字隐私                                                       |
| **开放标准文件格式** | 私有二进制/加密格式（`.cdoc`、Fernet 加密表格模板）                                                                        | **DOCX**（`customXml` 保留种子与字符 id，Word/WPS/LibreOffice 可直接打开编辑）、**SVG 单线字体**、**SVG 模板**（表格兼支持 **XLSX**）、**TOML** 配置、**ZIP** 打包                  |
| **编辑与排版完整度** | Ctrl+Z 提示"撤销尚未开发完毕"；无自动避头尾（仅警告）；表格溢出弹窗阻塞编辑；多处方向键/表格导航"尚未开发"                 | 完整命令式**撤销/重做**；真正**自动避头尾**（作文格支持标点悬挂）；表格溢出作为非阻塞排版策略（自动缩字 / 增行 / 截断）；完整光标与表格 `Tab` 导航                                  |
| **文档自包含**       | 模板、原点、旋转等分散在全局设置                                                                                           | 模板、书写原点、页面旋转与单字微调随文档保存，换机器打开输出一致                                                                                                                    |

---

## 相比原版多出的新功能

除还原原版核心书写流程外，本项目新增了多项原版不具备的能力：

### 1. 手写效果与字体增强

- **中英文 / 数字分开设置：** 英文与数字不仅能指定独立字体，还可单独设置**字号缩放、基线偏移、上下浮动范围与独立字符变形**，解决中英混排比例或基线不协调的问题。
- **连笔（Cursive）：** 同一字符内相邻笔画起止点距离较近时，自动将"抬笔-移动-落笔"转换为短曲线连笔，呈现更自然的快写连带感。
- **字符轻重与随机轻重：** 支持按单字设置 5 档落笔轻重（更轻Ⅱ/Ⅰ、标准、更重Ⅰ/Ⅱ）并叠加随机轻重波动（`press_jitter`）。
- **独立倾斜与尾部控制：** 将"字形倾斜幅度"与"字形扭曲"拆分为独立滑块（原版共用同一滑块），并支持自定义小尾巴删除概率与长度阈值。
- **单线 SVG 字体库与实时监听：** 本地字体以标准 SVG 字体存储（兼容 Inkscape Hershey Text），支持设置主目录与多个共享/只读目录，放入文件即自动刷新，无需重启；支持单字体或 ZIP 批量导入/导出。

### 2. 排版与编辑效率

- **自动避头尾与作文格分词：** 真正执行中文避头尾换行/悬挂标点；作文格可选启用 `jieba-rs` 中文分词，尽量保持整词不跨行拆开。
- **排版空白智能清理：** 一键"消除中文与英文/数字间空格""消除标点与正文间空格"。
- **左侧导航窗格：** 集成全文搜索结果跳转列表与全文缺字（`缺字`）统计定位列表，点击条目直接定位到页面并选中。
- **非模态查找与替换：** 顶部 Word 式查找替换栏，支持**区分大小写**、**全角/半角等同**，且替换后未改动字符保留原有手写随机数 id。
- **格式刷与浮动迷你工具栏：** 支持单击单次套用、双击锁定连续刷格式；选中文本旁提供便捷样式操作。
- **草稿自动保存与矢量 PDF 导出：** 自动保存 `draft.docx` 防丢失；支持直接生成矢量 PDF（可分别开关"打印文字"与"打印背景格线"）。

### 3. 模板与本地表格识别

- **纯本地表格识别（`chixie-rs-import`）：** 无需云端服务，本地直接从**矢量 PDF**（解析直线/矩形绘制指令）、**扫描件或手机照片**（自适应阈值、透视校正与线段聚合）识别网格、合并单元格与内边距，直接生成可编辑表格模板。
- **SVG 模板库与 Excel 双向互通：** 模板保存为真实毫米尺寸的 SVG（参数嵌入 `<metadata>`，背景图内嵌），支持 ZIP 导入/导出、表格模板与 `.xlsx` 双向导入导出，以及直接导入原版 `.cx` 模板。

### 4. 设备校准、调试与 CLI 工具

- **落笔校准向导：** 让机器试写一排不同 Z 轴深度的测试线，直观选出落笔恰当的一档并回写设备配置。
- **完善的固件与命令工具：** 表格化 Grbl `$$` 参数编辑器（含中文名称与 `$RST=*` 二次确认恢复）、自定义命令按钮组、控制台历史记录，支持安全导入原版 `config.ini` 与 `.cset`。
- **全功能命令行（`chixie-rs-cli`）：** 支持无界面批量排版生成 G-code、生成 PDF、串口/网络发送、TF 卡管理、表格识别以及用于抓取对比原版输出的 `fake-grbl` 仿真服务器。

---

## 核心功能一览

- **富文本编辑器：** 基于稳定字符 id 的富文本编辑，完整撤销/重做，字号、字体、下划线、删除线、倾斜、上标/下标、标题、颜色（对应多色换笔）、对齐方式、首行缩进、单/双页预览、显示框线/行号/页码；支持 DOCX 打开/保存、TXT（含 GB18030）导入、PDF 导出与打印。
- **页面模板系统：** 支持**信纸（单线/双线/多栏）**、**作文格**与**表格模板**（合并/拆分单元格、拖拽调整行高列宽、单元格数据源与批量名单建页）；支持文档拖拽或右键移动书写原点。
- **手写与字库：** 云端字体登录下载与本地缓存、多字体按权重混用（支持"换一组"）、主线/英文/符号/数字角色分配、缺字自动回退与高亮提示、字体统一尺寸/去重/异常宽度优化。
- **输出规划与设备控制：** 之字型/弓字型路径规划、奇数/偶数/全部页面 180° 旋转、多色换笔暂停提示、书写优化（路径简化/平滑）、巡边与绘制框架线、笔锋测试；USB 自动连接、点动/归零、多页非模态换纸提示、TF 卡上传书写、断线重连续写与右键指定字符断点续写。

---

## 技术栈

| 领域               | 选型与说明                                                                                                                                                               |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **语言与工程**     | **Rust**（Edition 2024，最低要求 `1.98`），Cargo Workspace 多 crate 架构（8 个独立 crate）                                                                               |
| **桌面 GUI 框架**  | **`gpui-kit`（`gpui` / `gpui-pre 0.3.11` + `gpui-component 0.9.0`）**，GPU 硬件加速渲染（Windows DX11/Vulkan、macOS Metal、Linux Vulkan/Wayland/X11）                    |
| **文档与办公格式** | 自研轻量 OOXML 引擎（`zip` + `quick-xml` + `encoding_rs`）处理 DOCX 与 TXT；`calamine` 读取 Excel；`rust_xlsxwriter` 导出 Excel；`lopdf` 读取矢量 PDF 指令与生成矢量 PDF |
| **中文排版与分词** | 自研排字引擎与避头尾规则；`jieba-rs`（feature `segment`）提供作文格中文分词                                                                                              |
| **图形与文件监听** | `image` 处理 PNG/JPEG 预览与光栅表格识别；`notify` 实时监听字体与模板库目录变化；`embed-resource` 嵌入 Windows 应用图标                                                  |
| **加解密与网络**   | `reqwest`（`rustls`）对接云端接口；`aes` / `cbc` / `hmac` / `sha2` / `base64` 实现字体解密与校验；自研只读 pickle opcode 解析器导入 `.cx`/`.cset`                        |
| **硬件通信与并发** | `serialport` 串口通信（CH340 / GD32 / 蓝牙 SPP）；TCP/Telnet 与 ESP3D HTTP；`async-channel` 驱动单设备 Actor 状态机                                                      |
| **系统集成与配置** | `toml` / `serde` / `serde_json` 读写配置与数据；`directories` 获取跨平台标准目录；`keyring` 对接系统凭据库                                                               |
| **命令行与测试**   | `clap`（derive）构建 CLI；`proptest` 属性模糊测试、Golden G-code 回归测试、内置仿真设备与 `CHIXIE_RS_AUTOTEST=1` 全流程 GUI 自动化测试                                   |

---

## 文件位置说明

### 1. 源码工程结构（Repository Layout）

```text
chixie-rs/
├── Cargo.toml                     # Workspace 成员、共享依赖与编译 profile 配置
├── crates/
│   ├── chixie-rs-core/            # 核心库：云端 API、会话、凭据、SVG 字体库、字体混用/修复、排版、手写/连笔/变形、配置与 ChiXie3 配置导入
│   ├── chixie-rs-doc/             # 文档模型：稳定字符 id、格式 run、段落、命令模式撤销/重做、DOCX/TXT/.chx 读写、查找替换
│   ├── chixie-rs-template/        # 页面模板：信纸/作文格/表格模板、单元格填充、SVG 库、ZIP/XLSX 互导、.cx 导入
│   ├── chixie-rs-import/          # 本地表格识别：从矢量 PDF、扫描件与照片提取表格边框与合并单元格
│   ├── chixie-rs-output/          # 输出流水线：路径规划、旋转/原点变换、多色换笔分段、G-code 生成、矢量 PDF 导出
│   ├── chixie-rs-device/          # 设备通信：串口/Telnet/ESP3D 传输、Grbl 流控与重同步、断连恢复 Actor、局域网发现
│   ├── chixie-rs-gui/             # 桌面客户端：gpui 界面、Ribbon/Backstage、文档编辑器、模板编辑器、设备面板、模拟后端与各平台资源（resources/）
│   └── chixie-rs-cli/             # 命令行客户端 chixie-rs-cli 与 fake-grbl 抓包仿真器
└── docs/                          # 设计文档、ChiXie3 功能对照（chixie3-features.md）、里程碑（reimplementation-plan.md）、协议（protocol.md）、真机检查清单（hardware-checklist.md）
```

### 2. 运行时本机文件位置

程序遵循各操作系统规范目录，不向系统目录或注册表写入杂项：

| 数据类型                                         | Windows                                        | macOS                                                   | Linux                                                      | 说明                                                                      |
| ------------------------------------------------ | ---------------------------------------------- | ------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------- |
| **主配置** (`settings.toml`)                     | `%APPDATA%\chixie-rs\config\settings.toml`     | `~/Library/Application Support/chixie-rs/settings.toml` | `~/.config/chixie-rs/settings.toml`（`$XDG_CONFIG_HOME`）  | 保存设备参数、页面默认设置、手写风格与界面偏好（**不含密码**）            |
| **自动保存草稿** (`draft.docx`)                  | 与 `settings.toml` 同目录                      | 与 `settings.toml` 同目录                               | 与 `settings.toml` 同目录                                  | 编辑器改动后自动保存；兼容读取旧版 `draft.chx`                            |
| **运行时缓存** (`previews/`, `template_assets/`) | 与 `settings.toml` 同目录                      | 与 `settings.toml` 同目录                               | 与 `settings.toml` 同目录                                  | 存放云端字体预览图与模板背景 PNG 资源                                     |
| **本地字体库**（默认）                           | `%USERPROFILE%\Documents\chixie-rs\Fonts\`     | `~/Documents/chixie-rs/Fonts/`                          | `~/Documents/chixie-rs/Fonts/`（`$XDG_DOCUMENTS_DIR`）     | 每个字体一个 `.svg` 文件；可在 **选项 › 字体** 迁移路径并添加额外共享目录 |
| **本地模板库**（默认）                           | `%USERPROFILE%\Documents\chixie-rs\Templates\` | `~/Documents/chixie-rs/Templates/`                      | `~/Documents/chixie-rs/Templates/`（`$XDG_DOCUMENTS_DIR`） | 每个模板一个 `.svg` 文件；可在 **选项 › 模板** 迁移路径并添加额外只读目录 |
| **账号密码**                                     | Windows 凭据管理器                             | macOS 钥匙串（Keychain）                                | Linux Secret Service（D-Bus）                              | 仅存入系统安全凭据库（服务名 `chixie-rs`），永不落盘为明文                |
| **离线演示模式数据**                             | `%TEMP%\chixie-rs-demo\`                       | `/tmp/chixie-rs-demo/`（`$TMPDIR`）                     | `/tmp/chixie-rs-demo/`                                     | 启用 `CHIXIE_RS_FAKE=1` 时独立隔离存放，不触碰真实设置与库目录            |
| **旧版 ChiXie3 导入源**                          | `%USERPROFILE%\ChiXie3\conf\`                  | —                                                       | —                                                          | 支持手动导入 `config.ini`、`.cx` 模板与 `.cset` 设备参数                  |

> 注：若早期版本在配置目录存有 `fonts/` 或 `templates.toml`，首次启动时会自动迁移至新的 `Fonts/` 与 `Templates/` 目录（旧 `templates.toml` 会保留为 `.bak`）。

---

## 硬件与连接说明

- **已验证硬件：** 带 CH340 USB 转串口芯片的 Grbl_ESP32 赤写主板（约 300 × 300 mm 行程），有线连接。
- **掉线自动恢复：** CH340 在电机大负载下偶发 USB 掉线，重连后串口号可能变化；建议端口保持"自动"，客户端会按 VID/PID 自动重新识别并提示续写。

---

## 下载安装

GitHub Actions 每次 CI 都会在 `windows-latest`、`macos-latest`、`ubuntu-latest` 原生构建并上传 `chixie-rs-<OS>-debug` 产物（含图形端 `chixie-rs` 与命令行 `chixie-rs-cli`）：

- **Windows：** 解压获取 `chixie-rs.exe` 和 `chixie-rs-cli.exe`，直接运行。
- **macOS：** 解压 `chixie-rs-macOS.tar.gz`，将 `chixie-rs.app` 拖入"应用程序"。因未进行 Apple 公证，首次启动需右键点击应用选择 **打开**（或执行 `xattr -dr com.apple.quarantine`）。
- **Linux：** 解压 `chixie-rs-Linux.tar.gz`，运行 `./chixie-rs/install.sh` 安装到 `~/.local` 并注册桌面菜单与图标（全局安装可使用 `PREFIX=/usr/local`）。

应用图标由 [make_icons.py](crates/chixie-rs-gui/resources/icon/make_icons.py) 从 [OriginIcon.png](docs/OriginIcon.png) 生成，生成产物已随仓库提交。

---

## 从源码构建与测试

需要 **Rust 1.98+**（Edition 2024）。

```sh
cargo build --workspace
cargo run -p chixie-rs-gui
```

Linux（Debian / Ubuntu）需先安装系统开发依赖：

```sh
sudo apt-get install -y pkg-config libudev-dev libdbus-1-dev libfontconfig-dev libfreetype-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
  libvulkan-dev mesa-vulkan-drivers libzstd-dev
```

运行测试与静态检查：

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Release 构建：

```sh
cargo build --workspace --release
cargo run -p chixie-rs-gui --release
```

---

## 无真机/无账号快速试用

使用离线演示模式启动桌面客户端（任意账号，密码 `demo`，内置模拟 Grbl 主板，数据隔离在临时目录）：

```sh
CHIXIE_RS_FAKE=1 cargo run -p chixie-rs-gui
```

常用环境变量：

| 变量                                      | 作用                                                                    |
| ----------------------------------------- | ----------------------------------------------------------------------- |
| `CHIXIE_RS_FAKE=1`                        | 启用离线演示后端与模拟 Grbl 设备，设置与库文件写入临时目录              |
| `CHIXIE_RS_AUTOTEST=1`                    | 在演示模式下自动运行全流程脚本化 GUI 验收测试，输出 `AUTOTEST_*` 后退出 |
| `CHIXIE_RS_SIM_DELAY_MS`                  | 模拟设备每行 G-code 的执行延迟（毫秒，默认 `3`）                        |
| `CHIXIE_RS_PERF=1`                        | 输出排版与每帧绘制耗时统计                                              |
| `CHIXIE_RS_ACCOUNT`、`CHIXIE_RS_PASSWORD` | 为命令行字体相关指令提供登录凭据                                        |

---

## 命令行（`chixie-rs-cli`）

执行 `chixie-rs-cli --help` 查看完整参数，核心子命令如下：

| 命令                                     | 说明                                                                            |
| ---------------------------------------- | ------------------------------------------------------------------------------- |
| `ports`、`discover`                      | 列出可用串口，或扫描局域网无线主板                                              |
| `send`、`stream`                         | 向主板发送单条指令，或流式发送 G-code 文件（支持串口与 `--ip` Telnet）          |
| `sd`                                     | 通过 ESP3D HTTP 管理主板 TF 卡文件（`ls` / `upload` / `rm` / `run` / `status`） |
| `fonts`、`font-check`                    | 列出本地/已收藏字体、浏览在线字体库或诊断字体下载解密                           |
| `write`                                  | 将文本排版并生成 G-code（支持输出到标准输出、文件或直接发送到主板）             |
| `pdf`                                    | 将文本排在 A4 横线纸上导出为矢量 PDF（支持 `--no-ink` / `--no-background`）     |
| `detect-table`                           | 本地识别 PDF 或图片（PNG/JPEG）中的表格边框与合并单元格                         |
| `style-template`、`import-style`         | 生成手写风格 TOML 模板，或从 ChiXie3 的 `config.ini` 转换风格配置               |
| `calibrate`、`capture-diff`、`fake-grbl` | 抓取对照、字体尺寸校准与模拟 Grbl TCP 服务端（用于抓包验证）                    |

---

## 相关文档

- [docs/chixie3-features.md](docs/chixie3-features.md)：ChiXie3 完整功能拆解、实现状态与优势对比
- [docs/reimplementation-plan.md](docs/reimplementation-plan.md)：架构设计与各里程碑规划
- [docs/protocol.md](docs/protocol.md)：设备串口/Telnet 协议与抓包 G-code 规范
- [docs/server-api.md](docs/server-api.md)：后端接口文档
- [docs/hardware-checklist.md](docs/hardware-checklist.md)：真机硬件联调检查清单
