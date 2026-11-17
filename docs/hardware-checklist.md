# Hardware checklist

Run on the real machine before a milestone that touches devices is called done (plan §7). Copy
the table into the milestone's notes, fill in the result and date, and file anything that fails.

**Setup:** Grbl_ESP32 ChiXie board (CH340), USB to the PC, 300 × 300 mm travel, pen holder with
a pen, A4 paper taped at the origin. Start `chixie-rs`, log in only if fonts must download.
Keep a hand near the power switch for the first run after any device-code change.

| # | Check | How | Expected | Result |
|---|---|---|---|---|
| 1 | Auto-connect | start `chixie-rs` with the board plugged in; then 断开, unplug, replug, 连接 | connects by itself within ~2 s of start; after 断开 it stays disconnected until 连接 | |
| 2 | Jog | jog pad: 1 mm and 10 mm in all four directions | head moves the stated distance and direction | |
| 3 | Pen up / down | 抬笔 / 落笔 | pen lifts clear of the paper / touches it | |
| 4 | Set origin | jog to a corner, 设为原点 | position reads X0 Y0 | |
| 5 | Short job | type `你好赤写 A1`, 开始书写 | written as on screen; pen parks at X0 Y0 at the end | |
| 5a | 写完位置 | 参数设置 › 写完位置 原点左侧 20 mm, write one line | pen parks 20 mm left of the origin **on the paper** (check with 坐标方向 as used) | |
| 5b | 书写此页 / 从此页起 | caret on page 2 of 3, each button once | only page 2 / pages 2–3 are written; progress names the real page | |
| 5c | 巡边 | 巡边 (one click), then 巡边方式 › 抬笔巡边（顶点落笔）· 实际书写区域 | pen-up trace around the text, no mark; second run dots the four corners | |
| 5d | 绘制框架线 | a 信纸 or 作文格 page, 绘制框架线 | lines land on the template positions; dashed helper lines come out dashed | |
| 6 | Multi-page job | 3 pages of text | sheet-change prompt between pages; resumes after 已换纸继续 | |
| 7 | Pause / resume | 暂停 mid-stroke, wait 5 s, 继续 | head stops at once; continues without a gap or duplicate stroke | |
| 8 | Stop | 停止 mid-job | head stops, pen lifts, parks at X0 Y0; no alarm left | |
| 9 | Alarm and unlock | trigger an alarm (e.g. soft-limit jog) | toast "设备报警：ALARM:n …" in Chinese with 解锁; 解锁 clears it | |
| 10 | USB drop | pull the USB cable mid-job, plug it back in within 60 s | toast with page and line; task pane shows 正在自动重连; reconnects by itself (COM number may change) | |
| 11 | Resume after drop | after #10: if the pane says the board rebooted, jog to the paper origin, 设为原点; then 继续书写 | toast names the character (将从「X」所在笔画继续); writing restarts at the stroke the pen was on: no stroke before it is drawn again, nothing missing (at most that one stroke is retraced) | |
| 11a | Drop during sheet change | pull USB while the 换纸 prompt is open, replug, 继续书写 | the 换纸 prompt comes back before the next page starts | |
| 11b | 断点续写 by character | 停止 mid-page; 书写 › 断点续写; the card pre-selects the remembered character; or select another single character; 开始续写 | continues that page from the character's first stroke, then the following pages with sheet prompts; selecting two characters or a space shows the ChiXie3 message and keeps 开始续写 disabled | |
| 11c | Resume, board kept power | pull only the USB data line (board powered separately) mid-page, replug, 继续书写 | no re-zero needed; resumes at the stroke the head stopped on (position read after reconnecting) | |
| 12 | Wireless | same board over Wi-Fi/Telnet (M1) | connects; a short job writes as over USB | |
| 13 | Templates | one 信纸 and one 作文格 page | text sits on the printed lines / in the cells (± 0.5 mm) | |
| 13a | Typesetting (M3) | 作文格 page with "，" and "。”" falling at row ends and "2026年"; 信纸 page with one 居中 title, one 右对齐 line and a Tab-indented paragraph | marks sit in the last cell's lower right, never at a row start; "20" / "26" share cells; title centred, right line ends at the margin, indent ≈ 16 mm; written as on screen | |
| 13b | Mixed fonts (M4) | 字体库: two fonts 加入混用 (weights 3 and 1), a third as 英文字体; write a page of Chinese with a few English words | Chinese characters alternate between the two fonts (about 3:1), English words use the 英文 font; 换一组 changes which character gets which | |
| 13c | Effects (M5) | 手写 › 字符变形 on with 字形变形幅度 10 and 随机删除小尾巴; 字符微调 one character (上移 ×2, A+); 书写优化 简化路径 | characters lean and bend slightly, each differently; the tuned character sits 0.4 mm higher and 5 % larger; the job runs as before with fewer G-code lines | |
| 14 | Long job | 20 pages, sheets changed promptly (M1 done-when) | completes with only sheet changes; no stalls, no drift between pages | |
| 15 | Wireless long job | ≥ 2000 lines over Telnet (where ChiXie3 stalls at ~44 / 752 / 1682) | completes; a lost ack shows the "已自动同步" toast instead of hanging | |

Notes:
- The CH340 board can drop off USB under motor load and come back on another COM number; use
  自动, not a fixed port. Power the board separately if drops repeat.
- Record the board's `$$` output once per firmware change (设备 › 参数设置 shows it).
