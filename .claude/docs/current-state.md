# Current state — chixie-rs (2026-11-07)

## Status
- Plan: `C:\Users\Symb0x76\.claude\plans\d-projects-rust-chixie-rs-docs-handoff-fluttering-iverson.md`.
- M0 done: workspace, golden, gpui-kit spike.
- M1 done: calibration (mm/unit = size/100) plus jitter, deformation and multi-line layout rules,
  all captured from ChiXie3 via a fake Grbl. See docs/protocol.md and memory `chixie3-capture-method`.
- M2 done (core side):
  - layout/gcode split; Grbl pause/status; device actor;
  - calibration; handwriting jitter; deformation; style TOML; CLI flags.
- Tests: 52 core (+1 opt-in keyring) + 2 goldens; GUI autotest passes; clippy clean.
- M9 Step 7 (`.cset` import) on `claude/upbeat-engelbart-aa9c11`: core `pickle.rs` (safe reader,
  shared with templates), `chixie3_devices::import_cset`, 从炽写导入 menu (config.ini | .cset) with a
  device picker. gpui's file prompt has no extension filter, so the picker is unfiltered. Workspace
  tests: 283 passed, 2 ignored; clippy clean. GUI dialogs not yet clicked through.

## Open / VERIFY
- Space width; wrap rule (glyph origin vs right edge); first-line indent mode; `row_offset_style`
  variants.
- Exact deformation stroke-shift and shape-noise formulas.
- On hardware: `?` polling while streaming, the stop sequence, the USB-drop path.
- ChiXie3's Telnet stalls on long jobs, so keep capture jobs short.

## Next
- Done since: config.rs + ChiXie3 importer, credentials.rs (keyring), API heartbeat/logout.
- M3 done: GUI shell, login (keyring remember-me), fonts browser; offline FAKE/AUTOTEST modes.
- M4 replaced by the Word-style WYSIWYG editor (plan revision 2026-11-07):
  - W1 core: `editor.rs` (EditDoc, undo, IME marked range, UTF-16), per-character-id randomness,
    CharBox/caret/hit-test in layout.
  - W2–W5 GUI: ribbon (文件/开始/页面布局/手写效果/设备/视图) + status bar; `doc_view.rs` page
    canvas (multi-page, zoom, caret, selection, mouse, keys, clipboard, undo, IME via
    `EntityInputHandler`); 字体库 opens FontsView in a Dialog.
  - W6: `file_ops.rs` (.chx with ids+seed, .txt import, G-code export), draft autosave
    (`draft.chx` next to settings, restored at start).
  - AUTOTEST covers IME composition/commit, Delete/Undo via dispatched actions, export, draft.
- Known/deviations: no gpui-component Select/Slider (unverified APIs) — font picker is a button
  opening the 字体库 dialog, zoom is −/+/100%/页宽 buttons. Debug-build stack: keep ribbon tabs in
  separate `#[inline(never)]` fns (one big fn overflowed the 1 MB main stack).
- Manual by BOSS: Microsoft Pinyin on the page, mouse selection feel, clipboard with other apps,
  real-account login.
- M5 done (UI side): 设备 ribbon tab (连接/断开, 开始书写, 暂停/继续, 停止, 已换纸继续, 面板开关),
  right task pane `device.rs` (port choice 自动/COMx ★, jog pad 0.1/1/10 mm, 抬笔/落笔, 设为原点,
  回原点, 解锁, live state + position, console, job progress), start confirm with extents
  (300 mm travel warning), 换纸 dialog, disconnect toast with page/line.
  FAKE mode uses `sim.rs` (simulated Grbl; `CHIXIE_RS_SIM_DELAY_MS`); AUTOTEST runs the
  2-page job incl. sheet change → Done.
- VERIFY on hardware (BOSS): connect COM4 via 自动, jog/pen, full job, pause (head stops),
  stop (pen lifts), USB unplug mid-job → toast + reconnect via 自动.
- After hardware test (2026-11-07), BOSS asked for three things; all are done:
  - The pen parks at X0 Y0 after a finished job (`device.rs` run_job).
  - Page templates:
    - `template.rs`: blank / ruled (fixed or random rows, dual line, columns) / grid (作文格);
    - `layout::Ruling` + per-page guides, which are never plotted;
    - `templates.toml`;
    - ChiXie3 `.cx` pickle import (`chixie3_templates.rs`).
    - Formats and VERIFY items are in docs/protocol.md.
  - Word-style shell:
    - quick access toolbar, filled 文件 tab → Backstage (新建 from template, 打开, 保存, 另存为, 导出,
      模板管理, 账户, 选项);
    - ribbon with large and small buttons, captions and a ↘ launcher (手写效果 ranges dialog);
    - 布局 tab with paper / orientation / margin menus and a template gallery;
    - font `Select`;
    - Word-blue accent (`ribbon::set_theme`).
  - AUTOTEST covers both templates and the backstage. Screenshots are in the scratchpad (t3-*.png).
- Template capture done (2026-11-07): ruled-row placement fixed to match, grid already matched.
- VERIFY still (not captured): random-row semantics,
  dual-line use, column fill order.
- Login is optional (2026-11-07):
  - The app opens straight to the editor.
  - Login is under 文件 › 账户 (or the 登录 button in the tab strip) and is only needed to
    download fonts.
  - Downloaded fonts are saved decrypted in `<config>/fonts/<id>.json` + `index.json`
    (`font_store.rs`). 字体库 has a 本地字体 tab (使用 / 删除), and the font picker lists local
    fonts plus collected fonts marked 云端.
  - The last font reloads from disk at start. Logging out keeps the document and the local fonts.
- Device: 参数设置 tab (profile, ChiXie3 import, $$ machine specs); Stop and Done both park at
  X0 Y0.
- Word 365 "Colorful" chrome (2026-11-07):
  - custom blue title bar (`appears_transparent`): app mark, quick access toolbar, centred
    "文档名 - 赤写", 登录/account, own min/max/close via `WindowControlArea` (Windows hit-test);
    the backstage keeps the bar.
  - light tab strip with plain 文件 tab; ribbon is a white rounded card; groups split by inset rules.
  - grey desk with drop-shadowed pages; 设备 task pane with titled header + close.
  - status bar: hover cells (字体 opens 字体库, 设备 opens the pane), job progress bar, Word-style
    two-sided zoom slider (100 % in the middle), click % to reset.
  - palette in `ribbon::chrome` (light + dark). AUTOTEST passes; clippy clean.
  - VERIFY (BOSS): window drag/snap/resize on the custom caption, maximise/restore icon swap,
    min size 760×480. Non-Windows: caption buttons rely on Windows hit-test areas only.
- Follow-up (2026-11-07): every `Switch` → Word-style `Checkbox` (accent via `tokens.primary`);
  font picker items are `font_preview::FontItem` (own-stroke preview of the name, or a sample
  when the font lacks a name character; 云端 badge), previews also in 字体库 rows, cached in
  `AppState.previews` and built off-thread. `字体 #N` cause: the last font is fetched at login
  before the font lists arrive, so the placeholder was saved; now `FontStore::rename` repairs
  names whenever a list arrives (BOSS's store has `1 → 字体 #1`; fixed on next login).
- 字体库 as a 3×3 card grid (2026-11-07): every tab pages by 9 (local/collected client-side,
  online server-side), no scrolling. Card = preview band + name/action + tags · 字数 · ★收藏;
  description as tooltip. Un-downloaded fonts: `cloud_preview.rs` fetches `font_preview` PNG
  off-thread, crops to ink, 120 px ink mask cached in `<fonts>/previews/<id>.png` (~8 KB),
  tinted per theme; skeleton while loading, "暂无预览" fallback. Font picker uses the same
  `font_preview::face`. New dep `image` (same instance as gpui's).
- Fixed: title bar buttons (QAT, 登录) were swallowed as caption drags on Windows — gpui's
  hit-test reports the Drag area if any ancestor hitbox is hovered; buttons now `.occlude()`.
- Platform-aware chrome (2026-11-07): Windows keeps the blue caption; macOS/Linux use
  gpui-component `TitleBar` (traffic lights / CSD buttons, Linux `window_border`, client
  decorations), a light caption, ⌘ shortcuts and macOS text navigation, keychain wording.
  Ctrl/⌘+S now saves (was only a tooltip). Windows autotest passes; macOS/Linux NOT compiled
  here (wayland pkg-config / aws-lc need native toolchains) — check on those machines.
- ChiXie3 feature spec (2026-11-07): `docs/chixie3-features.md` (140 features, status/priority/
  effort map, per-feature "recommended implementation", roadmap R1–R12, settings/endpoint
  appendices, conflicts). Evidence + extraction tools in `docs/chixie3-analysis/`.
  Designer labels of all 62 forms mined into `docs/chixie3-analysis/ui-labels.md` and merged
  (`[ui-form]` tag). 任务编程/固件烧录/多设备群控 are placeholders (no handler, inert as admin).
  Key conflicts to capture: 之/弓 path-planning meaning, `M3S` servo vs captured `G1G90 Z`,
  `G4 P` dwell after pen-down, 高矮 default 98–102 vs protocol.md.
- Reimplementation plan (2026-11-08): `docs/reimplementation-plan.md`. Product name **chixie-rs**
  (published company-wide; it was "SAST Writer" for a few hours on 2026-11-08). BOSS decisions:
  - writing first (Phase 1 = M0–M10, ~27–37 eng-weeks), then plotting/laser (Phase 2), 百宝箱
    tools (Phase 3), users' own fonts (Phase 4: handwriting capture + zi2zi-JiT/DA-Font generation
    → centre-line vectoriser; NO system-font scan);
  - common formats only: documents DOCX (+ customXml part for seed/ids/template), fonts SVG fonts
    (single-line), templates SVG (+ XLSX for tables), settings/profiles TOML, bundles ZIP;
  - English folder names: `Documents/chixie-rs/Fonts/`, `.../Templates/` (movable, extra folders,
    watched);
  - cloud = fonts + template library + 参数同步, local-first, everything exportable/importable;
  - no AI, no commerce, no `.cdoc`/encrypted ChiXie3 formats; local table detector; internal tool
    (no signing/installers/self-update); Windows/macOS/Linux from day one.
- **M0 done (2026-11-08), branch `m0-foundations`** (not merged; no git remote yet):
  - Naming: crates `chixie-rs-{core,cli,gui}`, binaries `chixie-rs` / `chixie-rs-cli`, env
    `CHIXIE_RS_{FAKE,AUTOTEST,SIM_DELAY_MS,ACCOUNT,PASSWORD}`, data dir and keychain service
    `chixie-rs` (unchanged from before, no migration). Hardware/ChiXie3 names unchanged.
  - CI `.github/workflows/ci.yml`: Windows/macOS/Linux build, clippy -D warnings, tests, GUI
    autotest (non-blocking until seen green), debug artifacts. NOT run yet: needs a GitHub remote.
  - Capture harness `tools/capture/` (`chixie-rs-cli fake-grbl`, `capture-diff`,
    `capture.ps1`): fresh un-elevated ChiXie3 per run; reproduces protocol.md structure exactly.
    Finding: pasted text varies 0.1–1.3 mm per stroke between identical runs (deform off).
  - Cloud API: `config/font_config` GET shape captured; ChiXie3 has no template-library backend;
    endpoint proposal in `docs/cloud-api-proposal.md` (needs backend team). `api_log.py` addon.
  - `docs/hardware-checklist.md` (14 checks).
- M0 merged to master (`a16c02c`), CI green on all three OSes.
- **M1 code done (2026-11-14), merged to master (`cc759b6`) before the hardware run:**
  - New crate `chixie-rs-device` (transport, grbl, device actor, esp3d, discovery, job).
    `DEFAULT_BAUD` lives in core `config`.
  - Stall fix: status replies used to reset the silence timer, so a lost `ok` hung forever. Now:
    no ack for 5 s + fresh `Idle` → resync (via `Bf:` or a `$G` marker), `Event::Resynced` toast.
  - Grbl error/alarm texts in Chinese (`grbl::error_text` / `alarm_text`); alarm toast with 解锁.
  - `Job.first_page`; ribbon 书写此页 / 从此页起 (caret page); `DeviceProfile.end_position`
    (写完位置: origin / origin-left 20 mm / stay / custom, through the axis map).
  - USB-drop recovery (BOSS decision: re-zero, then resume from a stroke): actor rescans 60 s,
    detects reboot (banner, or (0,0) after elsewhere), `Recovered { line, rebooted }`; resume line =
    pen-up ≥ 16 lines (planner depth) before the last ack; jog / 设为原点 work while waiting;
    a drop during the sheet prompt re-asks for the sheet.
  - 巡边 (3 modes × written/template area) and 绘制框架线 (merged, boustrophedon, dashes).
  - Auto-connect (`DeviceSettings.auto_connect`, 2 s poll, retries only when the port set changes).
  - Sim knobs `CHIXIE_RS_SIM_DROP_AT`, `CHIXIE_RS_SIM_DROP_OK`; autotest prints
    `AUTOTEST_DEVICE … recoveries=` and `AUTOTEST_PATROL`.
- Open: hardware checklist #1–#15 (BOSS), especially Telnet (#12, #15), the 20-page job (#14),
  and whether a real USB drop reboots the board (decides how often #11 needs a re-zero).
- Dependabot (2026-11-16): rustls 0.23.46, quinn-proto 0.11.19 (`7378993`), all 5 alerts fixed.
- **M2 done (2026-11-16), merged to master (`12ef047`)** (plan in `C:\Users\Symb0x76\.claude\plans\lexical-honking-thacker.md`):
  - Resume map: `layout::Page.stroke_chars` (char per stroke), `gcode::emit_mapped` → `PageCode
    { text, stroke_lines }`, `PageCode::from_stroke(k)` (pen-up + identical tail, remapped).
  - Line-exact resume: `Job.maps`; actor tracks work XY (WPos or MPos−WCO), and after a drop
    `device::resume_at` picks the stroke under the head among those the acks allow (earliest on
    crossing strokes, next one if at a stroke's end; 2 mm tolerance, else M1 `resume_point`).
    Board kept power → position read after reconnecting.
  - Fake Grbl now interleaves `?` replies with acks (it used to starve them).
  - GUI 断点续写 (`app/resume.rs`): card in the task pane, ChiXie3 validation texts, remembers the
    break character (by id) from stop/error/reconnect; autotest `AUTOTEST_RESUME`.
  - New crate `chixie-rs-doc`: `Document` (chars + ids + interned `Attrs` + `Para`), `Command`
    (Splice/SetAttrs/SetParas, apply returns inverse), `EditDoc` moved from core (undo = inverse
    commands; same API + `set_attrs`/`set_align`); proptest undo/redo property.
  - DOCX (`docx.rs`, zip 9 + quick-xml 0.41): runs/paragraphs for Word, custom XML part
    `urn:chixie-rs:doc:1` with seed/ids/exact attrs; prefix/suffix id recovery after outside edits;
    `.chx` v1 + old `draft.chx` migrate; draft is `draft.docx`; golden: G-code identical after a
    round trip; `AUTOTEST_DOCX`.
  - Attribute UI (font/size/colour per run) is NOT in M2: model + file only; layout still uses the
    document font/size (M3/M4/M6).
- Passed by BOSS 2026-11-17 (COM5 CH340, Grbl_ESP32 1.3a): hardware #1–#5, #10, #11, #11b,
  #11c; Word save/reopen round trip.
- Open (BOSS): .docx in WPS and LibreOffice; M1 checks #5a–#9, #12–#15 (Telnet, 20-page job).
- **M3 done (2026-11-17), merged to master (`6f018f0`)** (plan in `C:\Users\Symb0x76\.claude\plans\lexical-honking-thacker.md`):
  - `core/src/layout/` module dir (crate split deferred to M6 by BOSS): `mod.rs` (`layout_flow`
    with `Flow { chars, ids, paras, joins }`, `layout_doc`, `layout_ids` wrappers), `pen.rs`
    (line/page bookkeeping, `align_line`, `truncate_to`, `draw_char`, `glyph_for`),
    `kinsoku.rs`, `segment.rs` (jieba-rs behind core feature `segment`; GUI enables it).
  - `Align`/`Para` moved to core (`chixie_rs_doc` re-exports). Alignment per paragraph,
    explicit indent (`Para.first_indent`), Tab / 首行缩进 toggle (`HandwritingStyle::indent_width`).
  - 避头尾 (`PageParams.kinsoku`, settings `page.kinsoku` default on): main loop is `while` and
    steps back ≤ 3 chars; grid hangs ≤ 2 closing marks in the last cell. `Document.punct_issues`
    → status-bar badge (`NextPunctIssue`). Fuzz: `core/tests/kinsoku_fuzz.rs`.
  - Grid: two digits (letters with `grid_pair_letters`) per cell; 合并内容 via
    `Attrs.combine_next` + `EditDoc::toggle_combine`; half space U+2009 (Shift+Space);
    width fallback `glyph_for`.
  - GUI: 段落 group (4 align toggles, 首行缩进, 合并内容 grid-only), 布局 › 排版 checkboxes,
    autotest `AUTOTEST_TYPESET` (CI greps it).
  - Fixed a racy device test (`connects_and_jogs`).
- Known: 作文格 capture test stays at 0.7 mm (ink top 0.64 mm above capture; calibration task).
- Next: BOSS hardware #13a → M4 (fonts).
- **M4 done (2026-11-17), merged to master (`f876812`):**
  - core `font/` module dir: `crypto.rs` (Fernet), `svg.rs` (SVG font files, exact round trip), `library.rs`
    (`FontLibrary`, `FontKey` = `cloud:<id>` | `sha:<12 hex>`, `local_id()` ↔ i64 for the GUI, scan/import/
    export/move/migrate, `watch` via notify 8.2), `fontset.rs` (`Glyphs` trait, `FontRoles`, `MixEntry`,
    `FontSet`), `fix.rs` (字体优化).
  - Settings `fonts: FontSettings { library_dir, extra_dirs, roles, mix, mix_salt, fix }`; fake/autotest mode
    uses `<temp>/chixie-rs-demo/Fonts`, never Documents.
  - GUI: `font_lib.rs` (`LibraryStore`, cached scan, old FontStore API by i64), `app/fonts_ctl.rs`
    (reload_fonts → `AppState.font_set` + `font_set_gen`, roles, mix, watcher, import/export, folders,
    缺失字符 dialog), 字体库 card ⋯ menu + roles/mix bar, 选项 › 字体; autotest `AUTOTEST_FONTS`.
  - Dropped by BOSS: 收藏/取消收藏, 主字体 sync, 我的录制, 今日试用, all network capture (memory: no-network-analysis).
- Next: BOSS hardware #13a/#13b.
- **M5 done (2026-11-17), merged to master (`d39fa50`):**
  - `deformation.rs`: shape = slant + `Tps` warp (4×4 grid, hand-written solve, affine part dropped),
    `remove_tails` (随机删除小尾巴, drawn last), `Deformation::recommended()` (使用推荐参数).
  - `handwriting.rs`: `Jitter::vertical(id, x, row_w)`; 智能偏移 = per-row sweep keyed by the row's
    first char (`SALT_ROW_DRIFT`).
  - `layout::Tuning` + `Flow.tuning`; `draw_char(.., t)` moves / scales about the foot / re-rolls;
    doc `Attrs.tuning`, `EditDoc::{tune, tune_same_chars, tunings}`; GUI `app/tune.rs` card.
  - `path.rs` + `DeviceProfile.route` (书写优化) in `gcode::emit_mapped`; 设备 › 参数设置 menu.
  - Captures 2026-11-17 in `captures/m5/` (local); finding in protocol.md: ChiXie3 paste noise hides
    the shape level. Harness `tools/capture/capture.ps1` now maximizes ChiXie3, waits 30 s, `-Probe`,
    clicks measured at 3840×2160 150 % (tools/ is git-ignored).
  - Autotest `AUTOTEST_EFFECTS`; styled golden re-blessed once.
- Next: BOSS hardware #13a/#13b/#13c.
- **M6 in progress (2026-11-17), branch `m6-output`:**
  - New crate `chixie-rs-output`: `emit.rs` (moved from core `gcode.rs`; `PageCode.pens`, changer
    macros, `standalone` SD program, 笔锋 `taper_points`) and `plan.rs` (`plan` / `write` / `plan_all`
    → `Written { page, pens }`; 弓字型, 180° about the frame centre, origin shift, `Sheet::snap_origin`).
    Goldens + legacy oracle moved to `crates/chixie-rs-output/tests/`.
  - Core `profile.rs` (was `gcode.rs` types): `PathPlan`, `Origin`, `PageRotation`, `PenChange`/`PenSlot`,
    `TipTaper`, `Rgb`/`colour_name`; `PageSettings.{origin, rotation, multicolour}`; `num` maps -0 to 0.
    `Page::first_stroke_from` picks the character first (works on reordered pages).
  - Device: `Job.pens`, `Event::AwaitingPen` (after `G4 P0`), continue with `NextSheet`; `stream_range`;
    `esp3d::Board::upload_and_run`, `no_proxy`.
  - GUI: `ChixieRsApp::written_pages` + `app::job_of` feed every job/export/patrol; 参数设置 rows (路径规划,
    书写原点, 页面旋转, 多色绘制/换笔模式, 笔锋); draggable origin ring in `doc_view.rs`; 笔色 menu; coloured ink;
    换笔提示 dialog; `app/sd.rs` 上传SD卡; autotest `AUTOTEST_OUTPUT`.
  - Captured 2026-11-17 (`captures/m6/`, docs/protocol.md "Output options"): 弓 = row 2 right to left on
    ruled pages, no effect on 作文格 (`Document.grid` skips it); rotation 1 = odd, 2 = even, about the
    frame centre. Local harness gained `-Text2` (typed Enter) and `-StartX` (▶ moves per template).
  - Open: auto changer slot editor; hardware #13d/#13e; not captured: rotation mode 3, 起点模式.
- **M6 merged 2026-11-17** (`f9680d0`).
- **M7 split (BOSS 2026-11-18):** M7a tables, M7b table-frame detector, M7c local template library (no cloud).
- **M7a in progress (2026-11-18), branch `m7a-tables`:**
  - New crate `chixie-rs-template`: `template.rs`, `chixie3_templates.rs` (moved from core), `table.rs` (TableTemplate /
    Table / Cell / Span / DataSource / SameType / CellRange, merge / split / pages), `fill.rs` (data-source values,
    refresh, batch_pages, import_sheets), `xlsx.rs` (calamine 0.38). Core keeps the template name only;
    `Settings::page_params_with(Ruling)`, `Template::apply_to(&mut PageSettings)`.
  - Core layout: `Ruling::Table`, `CELL_BREAK` (U+001F), `CellBox` / `CellInfo` / `CellFit`, `Overflow`, `VAlign`,
    `Document.cells`; layout_flow fits Shrink / AddRows with extra passes; overflow clipped, never written.
    `Ruling::Rows` gained `widths` and `upper`.
  - Doc: `EditDoc::{next_cell, prev_cell, replace_many}`; DOCX `write_tables` / `TableShape` (w:tbl, gridSpan, vMerge),
    cell break outside tables written as a tab; `chixie_rs_doc::save` takes table shapes.
  - GUI: `templates_view/table.rs` (table editor), `table_ui.rs` (内容溢出, 批量溢出, 刷新数据, 批量导入), Tab / Shift+Tab,
    overflow badge, ribbon 表格功能, origin right-click menu, template save checks, per-column widths, upper band.
  - Autotest `AUTOTEST_TABLE` (grepped in CI). Not run locally (BOSS at the screen); CI runs it.
  - ChiXie3 2×2 form captured 2026-11-18 (`captures/m7/table22.gcode`): text centred in each cell row (was the note
    rule), 1 mm padding + ChiXie3's random 首字不对齐; test `table_cells_match_chixie3_capture`. The capture left a
    template 新建表格 in BOSS's ChiXie3.
  - Open: 绘制表格 drag, 同型 sync, 异栏同型, data-source prompt on new pages.
- **M7a merged 2026-11-18** (`bcbc50f`).
- **M7b in progress (2026-11-18), branch `m7b-detector`:**
  - New crate `chixie-rs-import`: `grid.rs` (segments → tables, weighted line positions, lines drawn along no span
    dropped), `pdf.rs` (lopdf 0.47 content walk, scans), `raster.rs` (Sauvola, components, corner refine by line
    fits, homography unwarp, ink runs; table frame vs sheet edge), `synth.rs` (test forms: PDFs, rendered photos /
    scans), `detect()`, `left_padding()`. Tests: `tests/forms.rs` (10 forms) + git-ignored `tests/forms-real/`.
  - CLI `detect-table [--json]`. GUI: `templates_view/import.rs` (import dialog, drop, backgrounds in
    `template_assets/`, 内边距识别, 透明度); `TablePage.background`. Autotest `AUTOTEST_DETECT` (CI uses grep -F).
  - Open: real forms from BOSS (photos with strong perspective are the risk), thumbnails, 截图生成, 占位符识别.
- **M7b merged 2026-11-18** (`fc2076d`).
- **M7c merged 2026-11-18** (`120ba7e`). Details:
  - Template crate: `svg.rs` (template ↔ SVG; TOML in `<metadata id="chixie-template">`, backgrounds as data URIs
    restored into the assets folder by FNV hash), `library.rs` (TemplateLibrary: scan / save / remove / sync /
    import SVG-ZIP-XLSX-cx / export SVG-XLSX-ZIP / move_default / migrate_toml / load_store), `xlsx_template.rs`
    (rust_xlsxwriter export; import reads sheet XML for widths / heights / merges); `Template.tags`, `.favourite`.
  - Core `Settings.templates` (`library_dir`, `extra_dirs`). GUI: `AppState.template_lib`, `save_templates` = sync,
    `app/templates_ctl.rs` (watch, folders, gallery `bs_gallery`, `gallery_list`), templates_view import / export /
    tags; 选项 › 模板. Demo / autotest use `<temp>/chixie-rs-demo/Templates`. `AUTOTEST_LIBRARY`.
  - Open: cloud library (M10), thumbnails, 同型 sync, 绘制表格 drag.
- **M8 (2026-11-22), branch `m8-editor`** (plan `C:\Users\Symb0x76\.claude\plans\lexical-honking-thacker.md`):
  - Core: `layout::CharStyle` / `Script` / `Flow.styles` (size ratio, 12° tilt, 0.6× scripts, 1.5× 标题, rule
    strokes per run per line in `pen.rs::rule_strokes`, salt `SALT_RULE`); `fontset::RunFonts` (per-char font by id).
    Factors are VERIFY (docs/protocol.md "Character formats"); goldens unchanged.
  - Doc: `Attrs.{tilt, underline, strike, script, title}` + DOCX (w:b/i/strike/u/vertAlign; left tilt custom only),
    `PT_PER_SIZE` (字号 100 = 20 pt), `EditDoc::{common, toggle_attr, clear_format, paint_format, styles, char_fonts,
    selected_attrs, insert_with_attrs}`, `find.rs` (FindOpts, find_all, replace_all), `import.rs` (decode_text with
    encoding_rs GB18030, docx_chars, import_chars 覆盖/追加/插入). Replacing keeps the replaced text's formats.
  - Output: `pdf.rs` (lopdf vector PDF; 打印文字 / 打印背景图). Import crate: `pdf::text`. CLI `pdf`.
  - GUI: `doc_view/format.rs` (actions, FormatState, 格式刷), ribbon 字体 row + painter, font / size boxes follow the
    selection (`sync_format_boxes`), `find_bar.rs`, rich clipboard (JSON metadata `RichClip`), page menu
    (`app/menu.rs`), 单页 / 双页 (`Geom.cols`), 行号 / 页码 (`paint_numbers`), `import_ui.rs` (+ drop), `app/print.rs`
    (export / print via the OS viewer). Settings `ui.{double_page, line_numbers, page_numbers, print_text,
    print_background}`. Autotest `AUTOTEST_FORMAT`; CI uploads `sample-pdf-<OS>`.
  - Open: ChiXie3 format capture (needs BOSS AFK), BOSS checks the 3 sample PDFs + checklist 13i.
- **M8 local GUI pass (2026-11-22, agent-desktop, BOSS AFK):** autotest green locally; formats, find / replace,
  context menu, 单页 / 双页, 行号 / 页码 driven by hand in FAKE mode. Fixed:
  - Paste lost formats when a clipboard tool (PowerToys / Windows clipboard history) re-puts plain text:
    `LastRichClip` global restores the last copy's attrs when the text matches. The autotest's `paste=` flaked
    false 4 of 5 runs on this machine before the fix.
  - 15 ribbon icons rendered blank (format buttons, align, indent, 合并内容, 字符微调): added to `icons.rs`.
  - Find bar pre-fill is now selected, so typing replaces it.
  - 页码 label moved into the gap under the page (it sat on the template's last ruled line).
  - 标题 (1.5×) glyphs overlap the line above on ruled templates (rows keep their pitch). BOSS: wait for the
    ChiXie3 capture (noted in `docs/protocol.md` › Character formats).
  - agent-desktop notes: the gpui window has no useful UIA tree; drive with `--headed mouse-click --xy` from
    screenshots (window origin + (9, 0)); `press shift+right` does not extend selections (use Shift+click);
    text goes in via `clipboard-set` + `press ctrl+v`.
