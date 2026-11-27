# SXP-Simon/Panache 调研与 PenKaku 单线字体/算术/特殊字符扩展参考

> **调研日期**: 2026-11-27
> **目标仓库**: `SXP-Simon/Panache` (License: `AGPL-3.0-only`)
> **适用项目**: `PenKaku` (License: `GPL-3.0-only`)

---

## 1. 调研结论摘要

`SXP-Simon/Panache` 是一个基于 Flutter + Rust (`handwriting-core`) 的手写页面渲染器，面向 **PNG/PDF 位图输出**。

- **不能直接用于“字体生成”或单线字体导出**：Panache 处理的是传统 TTF/OTF 轮廓字体，通过 `fontdue` 栅格化为灰度位图（`GrayImage`），**不生成 TTF/OTF 文件，也不输出写字机可用的单线矢量折线（`Vec<Vec<Point>>`）**。其依赖 `write-fonts` 仅用于从 `.ttc` 字体集合中按 index 拆包出单个独立字体字节流。
- **协议隔离要求**：Panache 为 `AGPL-3.0-only`，PenKaku 为 `GPL-3.0-only`。**禁止直接复制或链接 Panache 源码**，仅可借鉴其数据结构与几何算法思路在 PenKaku 中独立实现。
- **高价值可借鉴思路**：
  1. **Unicode 字符降维与规范化**：将减号变体、Unicode 上下标、Unicode 分数还原为字体中已有的基础字符，交给排版层缩放定位。
  2. **基于主字体几何特征的矢量补字**：在 PenKaku 现有的单线矢量层（延续 `penkaku-core/src/font/mod.rs` 中 `compose_arrows` 的模式），用基础笔画组合生成缺失算术符号与带圈数字。
  3. **轻量行内数学结构排版**：不引入重型 LaTeX 引擎，用有限 AST 节点（分式、根式、上下标）直接生成单线几何路径与缩放子文本。

---

## 2. Panache 核心实现剖析

### 2.1 字体加载与本地字体管理
- **`crates/handwriting-core/src/local_fonts.rs`**：
  - 使用 `fontdb` 扫描系统、用户和内置字体目录，用 `ttf-parser` 读取 `name` 表提取中英文家族名。
  - 遇到 `.ttc` 字体集合时，通过 `write_fonts::FontBuilder::new().copy_missing_tables(...)` 提取单 face 为独立 TTF/OTF 字节流。
- **`crates/handwriting-core/src/fonts.rs`**：
  - **真·可见字形校验 (`visible_glyph`)**：不仅检查 `lookup_glyph_index(c) != 0`，还会栅格化到 `36px` 检查是否存在 `alpha >= 128` 的像素，防止部分手写字体将数学符号映射为空白轮廓导致的“假存在”。
  - **按需 Fallback 链 (`FontBook::with_text_fallbacks`)**：
    - 花体大写字母（`𝒜..𝒵`）→ 回退至内置 `PanacheMathScript.ttf`。
    - 黑板粗体（`𝔸..𝕫`，`U+1D538..=U+1D551`）→ 回退至内置 `HandwritingMathSymbols.ttf`。
    - 其余缺失字符依次回退至内置中文手写体（霞鹜文楷、霞鹜漫黑、李国夫手写体、悠哉手写体）。

### 2.2 算术与特殊字符文本规范化
- **`crates/handwriting-core/src/text.rs`**：
  - `normalize_for_layout` 将手写字体极易缺失或无墨迹的数学标点映射到常用 ASCII：
    - `'−'` (U+2212) → `'-'`
    - `'∣'` → `'|'`，`'∥'` → `"||"`，`'∶'` → `':'`
  - `script_char` 将 Unicode 上标（`⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱˣʸᵏᵐ`）和下标（`₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓᵧ`）还原为普通字符 + 上/下标标记，避免依赖字体自身残缺的脚标字形。
- **`crates/handwriting-core/src/text/math.rs`**：
  - `vulgar_fraction` 将 Unicode 分数（`½ ⅓ ⅔ ¼ ¾ ⅕ ⅖ ⅗ ⅘ ⅙ ⅚ ⅛ ⅜ ⅝ ⅞`）直接拆解为 `Node::Fraction(分子, 分母)`。
  - 支持行内 `a/b` 识别（`slash_fraction`）和 `√` / `∛` / `∜` / `\sqrt[n]{x}` 识别（`radical`）。

### 2.3 程序化特殊字符绘制
- **`crates/handwriting-core/src/render/glyph/pen/sample.rs`**：
  - 从主字体采样 `'n'`, `'x'`, `'1'`, `'十'`, `'口'` 估算笔画宽度与 x-height、轴线高度、倾斜度（`slant`），采样 `'<'`/`'>'` 提取尖括号模板，采样 `'√'` 提取起笔/转折/根底比例（`RootShape`）。
- **`crates/handwriting-core/src/render/glyph/pen.rs`**：
  - **覆盖范围有限**：`MathPen::symbol` 仅硬编码补绘 10 个符号：`¬ → ← ↔ ⇒ ⇐ ⇔ ≤ ≥ ≠`（`≤`/`≥` 优先缩放主字体 `<`/`>` 并叠加横线）。
  - 常见算术符如 `± × ÷ ≈ ≡ ∑ ∫ ∂ ∞ ∈ ∉ ∪ ∩` 等**未做程序化生成**，仍依赖 TTF fallback。
- **`crates/handwriting-core/src/render/glyph/circled.rs`**：
  - 针对带圈数字 `⓪` 和 `①..㊿`：若主字体无有效字形（或与 `.notdef` 占位符一致），用 4 段三次贝塞尔曲线绘制手绘感歪椭圆外框，内部用主字体缩小绘制数字并居中组合。
- **`crates/handwriting-core/src/render/glyph/radical.rs`**：
  - 根据采样的 `RootShape` 比例用二次贝塞尔曲线绘制根号钩，并与水平顶线连接。

---

## 3. PenKaku 现状与可落地的执行方向

PenKaku（`crates/penkaku-core`）面向写字机 G-code 输出，字体模型为单线折线字形（`crates/penkaku-core/src/font/mod.rs` 中的 `Glyph { advance: f32, strokes: Vec<Vec<Point>> }`），并且已经实现了：
- `Font::compose_arrows`：利用字体自带的 `-`、`=`、`<`、`>` 合成缺失的 `→ ← ↔ ⇒ ⇐ ⇔`（支持单笔连笔优化）。
- `crates/penkaku-core/src/font/fix.rs`：单线字体尺寸统一、重复轨迹去重与异常宽度修复。

基于现有架构，建议按以下三个方向推进（供 Claude 后续设计与实施）：

### 方向 A：文本输入层做 Unicode 算术/脚标规范化（低成本、高收益）
在进入字形查表与排版前，对输入文本做无损降维映射，消除生僻码位缺字问题：
1. **符号统一**：`−` (U+2212) → `-`、`×` 可按需保留或映射、`∶` → `:`、`∣` → `|`、`∥` → `||`、`•`/`·` 等标点归一化。
2. **Unicode 上下标拆解**：将 `⁰¹²...` 与 `₀₁₂...` 拆为普通字符序列 + 上标/下标样式属性（在 `penkaku-doc` / `penkaku-core/src/layout` 中按比例缩小并上下偏移），无需在单线字库中单独造上下标字形。
3. **Unicode 分数拆解**：将 `½..⅞` 拆为 `1/2` 或结构化分式节点。

### 方向 B：扩展 `penkaku-core/src/font` 的单线矢量合成字符（延续 `compose_arrows`）
在 `Font` 加载或补字阶段，利用字体自身已有基础笔画的边界框（`bounds`）与中位高度，程序化合成缺失的常见算术与特殊符号（保持手写斜度与单线风格一致）：
1. **关系与算术运算符**：
   - `≠`：复用 `=` 的两横，按中心比例叠加一条斜杠 `/`（或从左到右倾斜穿线）。
   - `≤` / `≥`：复用 `<` / `>`，下方叠加一条略微倾斜或水平的手写底线（参考 `=` 或 `-` 的横线宽度与斜度）。
   - `±` / `∓`：复用 `+` 与 `-` 垂直紧凑组合。
   - `≈` / `≡`：复用 `~` 或 `=` 堆叠。
2. **带圈数字（`⓪`、`①..㊿`）**：
   - 借鉴 Panache `circled.rs` 的 4 段歪椭圆贝塞尔采样逻辑，直接在单线矢量层生成闭合/微缺口手写椭圆环（`Vec<Point>`），内部嵌入缩小居中的字体数字笔画，输出为标准 `Glyph`。
3. **单线根号（`√`）与基础几何框**：
   - 当字库缺少 `√` 时，按字高比例用二次贝塞尔曲线生成单线一笔根号折线。

### 方向 C：轻量行内数学结构排版（可选进阶）
如果后续需要在文档模型（`penkaku-doc`）和排版器（`penkaku-core/src/layout`）中支持行内算术式书写（分数线、嵌套根号、`^{...}`/`_{...}`）：
- 参考 `Panache` `text/math.rs` 的轻量 AST（仅 `Fraction` / `Radical` / `Scripts`，不引入完整 TeX 引擎），在布局阶段计算子节点的包围盒与基线偏移，分数线和根号顶线直接生成单线 `Stroke` 交给 `penkaku-output` 导出 G-code。

---

## 4. 开源字体资产说明（若考虑扩充基础字库）

Panache 仓库 `crates/handwriting-core/assets/` 与 `assets/font_assets/` 中包含多款开源字体（如 `Handlee`, `PatrickHand`, `IndieFlower`, `Kalam`, `ShadowsIntoLight`, `PanacheMathScript.ttf`, `HandwritingMathSymbols.ttf` 等）。
- **注意**：这些均为**闭合轮廓 TTF**，无法直接被 PenKaku 的单线写字机驱动使用。若未来需要从中转换字符到 PenKaku 字库，需要另外引入**中心线提取 / 骨架化（Centerline / Skeletonization）**工具链，Panache 本身不提供该能力。
