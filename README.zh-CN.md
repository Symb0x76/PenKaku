[English](README.md) | **简体中文**

# chixie-rs

赤写笔式手写机（Grbl_ESP32 主板）的桌面客户端，用 Rust 编写，用来替代 ChiXie3 Windows 客户端。
输入文档，套上模板、用手写字体排好版，机器就会用笔写出来。

内部工具，支持 Windows、macOS 和 Linux。字体来自你的赤写账号。

## 功能

- **编辑器：** 富文本，每个字有稳定 id；撤销、查找替换；格式（下划线、倾斜、上标下标、标题）；
  打开和保存 DOCX，导入 TXT，导出 PDF 和打印。
- **排版：** 信纸、作文格和表格模板，中文标点规则，对齐、缩进、页码。
- **模板：** 模板库（ZIP 导出导入）；表格模板可与 Excel 互相导入导出；从 PDF、扫描件和手机照片识别表格；
  导入 ChiXie3 的 `.cx` 模板。
- **手写效果：** 多字体按权重混用，单独的英文字体，手写抖动，字符变形，单字微调。
- **中英文分开设置：** 英文字母和数字可以单独设置大小、基线、上下浮动和变形，与中文互不影响
  （ChiXie3 没有这项功能）。
- **输出：** 笔画排序，弓字型路径，页面旋转，多色绘制（换笔提示），路径简化，框架线，巡边。
- **设备：** USB 串口自动连接，点动，抬笔落笔，设为原点；多页任务带换纸提示；暂停继续；
  USB 断开后自动重连并续写；从指定字符断点续写。
- **设备扩展：** Grbl `$$` 固件参数编辑，自定义命令按钮，控制台历史，导入 ChiXie3 的 `config.ini` 和 `.cset`。
- **性能：** 长文档输入和滚动不卡。16 页带手写效果的文档，重新排版约 40 ms，每帧绘制 3–5 ms（CI 调试版）。

## 硬件

测试机型：带 CH340 USB 串口芯片的 Grbl_ESP32 赤写主板，行程约 300 × 300 mm，用数据线连接。
CH340 在电机负载下可能掉线，重连后串口号会变；端口请保持“自动”，客户端会自己找回来。

## 下载

GitHub Actions 每次 CI 都会上传 Windows、macOS、Linux 的 `chixie-rs-<OS>-debug` 构建
（图形界面 `chixie-rs` 和命令行 `chixie-rs-cli`）。

## 从源码构建

需要 Rust 1.98 或更新版本（edition 2024）。

```sh
cargo build --workspace
cargo run -p chixie-rs-gui
```

Linux 需先安装系统依赖（Debian / Ubuntu）：

```sh
sudo apt-get install -y pkg-config libudev-dev libdbus-1-dev libfontconfig-dev libfreetype-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
  libvulkan-dev mesa-vulkan-drivers libzstd-dev
```

检查：

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 没有机器或账号时试用

```sh
CHIXIE_RS_FAKE=1 cargo run -p chixie-rs-gui
```

`CHIXIE_RS_FAKE=1` 使用离线演示后端（任意账号，密码 `demo`）和模拟的 Grbl 主板。
设置写到临时目录，不会动你真实的设置和已保存的密码。

其他开关：

| 变量 | 作用 |
|---|---|
| `CHIXIE_RS_AUTOTEST=1` | 在演示后端上跑脚本化 GUI 测试，输出 `AUTOTEST ...` 行后退出 |
| `CHIXIE_RS_PERF=1` | 记录排版和绘制耗时 |
| `CHIXIE_RS_ACCOUNT`、`CHIXIE_RS_PASSWORD` | 命令行字体相关命令的登录信息 |

## 命令行

`chixie-rs-cli --help` 列出全部命令。常用的：

| 命令 | 作用 |
|---|---|
| `ports` | 列出串口 |
| `send`、`stream` | 向主板发送一条命令，或流式发送 G-code 文件 |
| `fonts` | 列出已收藏的字体，或浏览在线字体库 |
| `write` | 把文字生成 G-code，打印出来或直接发给主板 |
| `pdf` | 把文字排在 A4 横线纸上并保存为 PDF |
| `style-template`、`import-style` | 生成书写风格配置，或把 ChiXie3 的 `config.ini` 转换成风格配置 |

## 工程结构

| Crate | 内容 |
|---|---|
| `chixie-rs-core` | 后端 API、字体、排版、手写效果、设置、pickle 读取 |
| `chixie-rs-device` | 串口和 Telnet 连接，Grbl 流式发送，设备 actor |
| `chixie-rs-doc` | 文档模型，可撤销命令，DOCX |
| `chixie-rs-template` | 页面模板，表格，ChiXie3 `.cx` 导入 |
| `chixie-rs-import` | 从 PDF、扫描件和照片识别表格 |
| `chixie-rs-output` | 笔画顺序，页面变换，笔色，G-code |
| `chixie-rs-gui` | 桌面应用（gpui + gpui-component） |
| `chixie-rs-cli` | 命令行 |

## 文档

- [docs/chixie3-features.md](docs/chixie3-features.md)：ChiXie3 功能及在本项目中的实现状态
- [docs/reimplementation-plan.md](docs/reimplementation-plan.md)：里程碑
- [docs/hardware-checklist.md](docs/hardware-checklist.md)：真机检查清单
- [docs/protocol.md](docs/protocol.md)：设备协议和 G-code 说明
