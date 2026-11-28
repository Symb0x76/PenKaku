# 从 inko-skill 可借鉴的设计与功能提案

Status: proposal, 2026-11-28.  
Reference: [pretend1111/inko-skill](https://github.com/pretend1111/inko-skill)（面向 AI Agent 的手写页面生成 Skill 与本地后处理工具链）。

## 背景与定位差异

PenKaku 与 `inko-skill` 解决的是不同层面的问题：

- **PenKaku**：本地优先、跨平台的写字机/绘图机桌面端与 CLI，核心链路是 **DOCX 富文本编辑（`penkaku-doc`）→ 模板/网格/表格排版与确定性形变（`penkaku-core`、`penkaku-template`）→ GRBL/CH340 串口流式写字（`penkaku-device`）+ 矢量 PDF 导出（`penkaku-output`）**。
- **inko-skill**：将云端手写生成 API（Inko Lyric 1 / Logic 1）包装成 AI Agent Skill，并围绕“**让输出更像真人手写作业/笔记**”与“**局部无损精修**”提供了一套轻量、可脚本化的本地后处理与排版规范。

PenKaku 在物理硬件控制、确定性种子形变（`SplitMix64` + TPS）、连笔、Z 轴轻重、本地表格检测等方面已经领先；`inko-skill` 值得借鉴的，主要集中在**拟真排版细节、自动化体检/批处理、文本手写化规则、以及 AI Agent 友好的 CLI/Skill 封装**。

---

## 1. 自然左边缘连续漂移（Natural Line Drift）

### 参考来源
`inko-skill` 的 `compose.py drift` / `scene.py drift`（`natural` 模式）：真实手写时，每行起点不会像打字机那样对齐到同一条垂线，也不会像独立随机数那样毫无关联，而是**随着行数向下缓慢向右漂移（creep）并叠加小幅随机抖动，在遇到空行或新题号时回弹（ease back）**；同时明确禁止在作文纸/田字格中启用。

### PenKaku 现状
[handwriting.rs](file:///d:/Projects/Rust/penkaku/crates/penkaku-core/src/handwriting.rs#L94-L96) 中 `HandwritingStyle::line_start_offset` 目前为每行独立采样（`SALT_LINE`），缺乏跨行惯性与段落/空行回弹。

### 建议方案
1. 在 `penkaku-core` 的 `HandwritingStyle` 中新增漂移模式（保留现有 `line_start_offset` 兼容）：
   - `LineStartMode::Independent`（默认，保持与 ChiXie3 兼容及 golden 测试一致）；
   - `LineStartMode::NaturalDrift { creep_mm_per_line: f32, wobble_mm: f32, max_drift_mm: f32 }`。
2. 在 `penkaku-core/src/layout/pen.rs` 换段/换行时维护当前漂移累积值：
   - 遇到空行、新段落或匹配题号前缀（如 `1.`、`(1)`、`第 N 题`）时将累积漂移按比例回弹（例如乘以 `0.3` 或重置）；
   - `Ruling::Grid`（作文纸/田字格）和 `Ruling::Table` 自动跳过该效果，避免字符偏出格框。

---

## 2. 整页排版“体检 + 一键收紧 / 基线对齐”（Inspect / Tighten / Align-Baseline）

### 参考来源
`inko-skill` 的 `scene.py` 针对“字写对了但局部版面不自然”提供了 3 个高频局部批处理命令：
- `inspect`：扫描全页异常宽的字间隙（`--max-gap`）、出界或重叠字符，输出 `anomalies` 与位置索引；
- `tighten`：自动将选中行或整页中超过阈值的宽间隙收紧到合理范围；
- `align-baseline`：按 `strength`（0.0–1.0）将偏离行基线过多的字向平均基线拉回，减弱过度抖动。

### PenKaku 现状
PenKaku 已支持“缺字”侧边栏跳转（`penkaku-gui`）和单字微调 [Tuning](file:///d:/Projects/Rust/penkaku/crates/penkaku-core/src/layout/mod.rs#L49-L62)（`dx_mm`、`dy_mm`、`scale`、`width`、`reroll`），但缺少**整行/整页级的排版异常自动检测与批量微调工具**。

### 建议方案
1. **排版体检（`penkaku-core::layout`）**：基于 `Document` 中的 `Page`、`LineInfo`、`CharBox` 遍历计算：
   - 相邻非空格字符的实测墨迹间距（ink bbox gap）大于阈值（如 `0.35 * em`）；
   - 字符超出右侧/下侧版心或与相邻行发生纵向碰撞；
   - 在 GUI 导航面板中与“缺字”并列展示“排版异常”跳转列表。
2. **批量微调写入 `Tuning`**：
   - **收紧大空隙**：对选中行或整页计算异常间隙，将多余偏移量累加到后续字符的 `Tuning::dx_mm`（通过现有 Undo/Redo 命令模式提交，可随时撤销）；
   - **减弱基线抖动**：将选中行内每个字符相对于行基线的 `dy_mm` 按 `(1.0 - strength)` 缩放。

---

## 3. “像学生手写”的文本清洗与数学规范

### 参考来源
`inko-skill` 的 `references/writing-text.md` 与 `references/writing-math.md` 总结了一套面向手写真实感的文本预处理经验：
- **Markdown / 富文本去排版化**：剥离 `#`、`**`、`>`、`---`、脚注与 emoji；将无序列表转为 `·` / `-` 或短行；
- **作业/解题模式**：数学推导与答题中**去掉句尾句号（`。`、`．`）**、仅保留句中逗号；`解：`、`所以`、`得` 后紧接公式不单独换行；
- **长公式智能折行**：长等式链不在写字机/纸面上强行缩小，而是在 `=`、`+`、`≤` 等运算符处自动断行；
- **LaTeX 手写子集与等价替换表**：整理了 `\geqslant` → `\geq`、单位写入公式（避免 `$x$ cm` 被误排为上标）等规则。

### PenKaku 现状
PenKaku 的“文本清理”（见 [doc_view.rs](file:///d:/Projects/Rust/penkaku/crates/penkaku-gui/src/doc_view.rs#L664)、[ribbon_tabs.rs](file:///d:/Projects/Rust/penkaku/crates/penkaku-gui/src/app/ribbon_tabs.rs#L284-L303)）已支持去中英空格、标点半角化、`->` 转箭头；[compose.rs](file:///d:/Projects/Rust/penkaku/crates/penkaku-core/src/font/compose.rs) 支持用字体自身笔画合成缺失的数学单字符号（`≤ × ÷ √ ①..㊿` 等）。

### 建议方案
1. **扩展“文本清理”选项**（`penkaku-doc`）：
   - 增加“清理 Markdown 标记”（去除 `#`、`**`、引用符 `>`、水平线及常见 emoji）；
   - 增加“解题/手写模式（去句末句号）”，去掉段落与行尾的 `。`；
   - 增加“复制/抄写保护”提示（抄写场景不触发自动标点替换）。
2. **后续二维公式排版参考**（若规划支持 `$...$`）：可复用其 LaTeX 常用子集边界与 `=`/`+` 处折行策略，配合 PenKaku 现有的 `Font::compose_math` 保持全篇笔迹一致。

---

## 4. 把 PenKaku 包装为 AI Agent Skill（`SKILL.md` + CLI `--json`）

### 参考来源
`inko-skill` 的核心模式是“**结构化 JSON CLI + 极简 `SKILL.md` 指南**”：任何支持 Skill 的编码助手（Claude Code、Codex、Cursor、Gemini CLI 等）都可以通过调用本地 CLI 完成“内容准备 → 校验缺字与页数 → 渲染预览 → 交付”。

### PenKaku 现状
[penkaku-cli](file:///d:/Projects/Rust/penkaku/crates/penkaku-cli/src/main.rs) 已经具备离线生成 G-code（`write`）、导出矢量 PDF（`pdf`）、表格检测（`detect-table`）及串口/Telnet 流式写字（`stream`）能力，其中 `detect-table` 已经支持 `--json`。

### 建议方案
1. **为 `penkaku-cli` 补齐 `--json` 结构化输出**：
   - `write --json` / `pdf --json`：返回 `{ "pages": N, "missing_chars": [...], "overflow_lines": [...], "out_path": "..." }`，方便上层脚本或 AI Agent 自动判断是否需要换字、调字号或缩减内容；
   - `write --dry-run --json`：仅执行排版与缺字校验，不写文件也不连接串口。
2. **提供仓库内置 `SKILL.md`**（例如 `skills/penkaku-plotter/SKILL.md`）：
   - 说明如何调用 `penkaku-cli` 完成“题目解答/笔记整理 → 文本清理 → `--dry-run --json` 校验缺字 → 导出 PDF 预览 → 确认后串口/Wi-Fi 写字机输出”；
   - 形成与 `inko-skill` 的本质差异：**不仅能生成手写图片/PDF，还能直接驱动真实写字机落笔到纸面**。

---

## 5. 命令行/自动化用的“行号与文本匹配”样式简写

### 参考来源
`inko-skill` 的 `layout.md` 在纯文本之外支持通过轻量 JSON `marks` 指定局部样式：
- 按段落索引：`{"line": [1, 2], "f": {"indent": 3}}`（第 1–2 段缩进 3 字，`-1` 表示末段）；
- 按文本匹配：`{"match": "此致", "nth": 1, "f": {"indent": 0}}`；
无需人工计算 UTF-8/UTF-16 字符偏移，即可覆盖对齐、缩进、字号缩放或字体切换。

### PenKaku 现状
GUI 中富文本格式完整保存在 DOCX（`customXml`），但 `penkaku-cli write` / `pdf` 目前仅接收纯文本 `text` 与全局 `--style` TOML，无法在命令行下对标题居中、落款右对齐等做轻量局部修饰。

### 建议方案
在 `penkaku-cli`（及可选的 `penkaku-doc` 导入层）支持可选的 `--marks <marks.toml|json>` 参数，在构造内存 `Document` 时将 `line` / `match` 规则映射为段落 `Align`、`first_line_indent` 或局部 `CharStyle`（缩放、轻重等），服务于自动化脚本与第 4 点的 Agent Skill 场景。

---

## 6. 字体几何特征标签 & 同文拼图预览（Contact Sheet）

### 参考来源
`inko-skill` 的 `styles.md` 为每个字迹维护 0–100 的多维特征标签（`neat` 工整、`joined` 连笔、`slant` 倾斜、`aspect` 宽高比、`round` 圆润度、`weight` 粗细、`compact` 紧凑度），并支持按条件筛选后生成 3–4 个候选字体的横向拼图（Contact Sheet）供人眼挑选。

### PenKaku 现状
[font/library.rs](file:///d:/Projects/Rust/penkaku/crates/penkaku-core/src/font/library.rs) 已通过扫描本地 SVG 文件夹与后台缓存字体预览 PNG 管理字体库，但缺少基于笔画几何特征的筛选维度和同文本多字体并排对比功能。

### 建议方案
1. **离线计算 SVG 字体几何标签**：在加载或扫描本地 SVG 字体时，抽样常用汉字（如 50–100 个高频字）的笔画直接计算：
   - 平均宽高比（`aspect`）；
   - 竖/撇笔画的平均倾角（`slant`）；
   - 笔画点数/总长度与外接框占比（`compact` / 复杂度）；
   - 连续笔画起止点距离分布（天然适合的 `joined` 连笔倾向）。
   无需人工标注或云端依赖，即可在 GUI 字体选择器中支持按“方正/圆润、端正/倾斜、紧凑/舒展”筛选。
2. **同文多字体对比预览**：在 GUI 字体库弹窗或 `penkaku-cli` 中提供“用同一句预览文本（默认取当前文档首行）渲染 3–4 个候选字体并排展示”的视图，替代逐个切换字体再回编辑器看效果的流程。

---

## 落地优先级建议

| 优先级 | 方向 | 涉及 Crate | 预期收益与工作量 |
|---|---|---|---|
| **P0** | 1. 自然左边缘连续漂移 | `penkaku-core` | 改动极小（数十行），直接提升非网格纸张手写自然度 |
| **P0** | 3. Markdown 清洗 & 解题去句号 | `penkaku-doc`、`penkaku-gui` | 复用现有“文本清理”入口，立刻改善 AI/网页复制文本的排版体验 |
| **P1** | 4. `penkaku-cli --json` + `SKILL.md` | `penkaku-cli`、`docs/` / `skills/` | 低成本打通 AI Agent → 真实写字机工作流 |
| **P1** | 2. 排版异常体检 + 一键收紧/基线拉平 | `penkaku-core`、`penkaku-doc`、`penkaku-gui` | 补齐整页/整行级批量微调能力，减少逐字 `Tuning` 操作 |
| **P2** | 5. 命令行 `line` / `match` 局部样式简写 | `penkaku-cli`、`penkaku-doc` | 配合 CLI 自动化与 Agent Skill 使用 |
| **P2** | 6. SVG 字体几何特征标签 + 同文对比预览 | `penkaku-core`、`penkaku-gui` | 提升多字体管理与选字体验 |
