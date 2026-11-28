---
name: penkaku-plotter
description: Turn text (answers, notes, letters) into single-line handwriting with PenKaku's CLI, check it for missing glyphs and overflow, preview it as PDF, then write it on paper with a GRBL pen plotter (写字机). Use when the user wants handwritten output from a font, a handwriting PDF, or to drive a pen plotter.
---

# PenKaku pen-plotter handwriting

`penkaku-cli` (build with `cargo build --release -p penkaku-cli`) lays text out with a
single-line handwriting font and writes G-code or a vector PDF. Unlike image generators it can
also drive a real plotter over serial or Wi-Fi (telnet).

## Font

Offline commands take a decrypted glyph JSON font: `--font-json <file>`. `write` can also use a
collected online font (`--font-id N`, login via `CHIXIE_ACCOUNT` / `CHIXIE_PASSWORD`). The repo's
test font `crates/penkaku-core/tests/fixtures/font_fixture.json` has only a few glyphs: use it
for smoke tests, not real output.

## Workflow

1. **Prepare the text.** Write it as a person would by hand: no Markdown (`#`, `**`, `>`, `---`,
   links, emoji), plain lists, and for worked maths answers no `。` at the end of a paragraph.
   Separate paragraphs with `\n`; a blank line or a question number (`1.`, `(2)`, `第3题`)
   starts a new block.
2. **Check it** (lays out only, writes nothing, touches no device):

   ```sh
   penkaku-cli write "$TEXT" --font-json font.json --font-size 100 --dry-run --json
   ```

   Output: `{"pages": N, "missing_chars": [...], "overflow_lines": [{"line", "page", "text"}], "out_path": null}`.
   - `missing_chars` not empty: the font lacks them. Rephrase, swap to a full-width / half-width
     variant, or pick another font. Never send text with missing glyphs to the plotter without
     telling the user: those characters are left blank on paper.
   - `overflow_lines` not empty: a line runs past the right edge. Shorten it or lower
     `--font-size`.
3. **Preview** on a ruled A4 page and show the PDF to the user:

   ```sh
   penkaku-cli pdf "$TEXT" --font-json font.json --font-size 100 --rows 24 --out preview.pdf --json
   ```

   `pdf --json` reports the same fields for the ruled page (here `overflow_lines` and `pages`
   reflect the A4 layout).
4. **Write**, only after the user confirms the preview and the paper is loaded:

   ```sh
   penkaku-cli write "$TEXT" --font-json font.json --font-size 100 --out page.gcode --json
   penkaku-cli stream page.gcode --port COM3        # or --ip 192.168.4.1 for Wi-Fi
   ```

   `write --port/--ip` streams directly. List ports with `penkaku-cli ports`, find Wi-Fi boards
   with `penkaku-cli discover`. Streaming moves real hardware: confirm with the user first.

## Local formats (`--marks`, `write` and `pdf`)

Centre a title, right-align a signature or indent paragraphs without character offsets:

```json
[{"line": 1, "f": {"align": "center", "size": 1.3}},
 {"match": "此致", "nth": 1, "f": {"indent": 0}},
 {"line": -1, "f": {"align": "right"}}]
```

`line` = paragraph number from 1 (`-1` = last) or a `[first, last]` range; `match` = text (every
occurrence, or only the `nth`). Formats: `align`, `indent` (characters), `size`, `underline`,
`strike`, `tilt`. TOML files use `[[mark]]` tables. Alignment needs a page, so it shows in `pdf`.

## Style (`write` only; `pdf` uses the default style)

- `--style style.toml`: handwriting jitter and deformation. `penkaku-cli style-template` prints a
  starting profile; `import-style` converts ChiXie3's `config.ini`.
- Natural left-edge drift: in the profile set
  `[handwriting.line_start_mode.natural_drift]` with `creep_mm_per_line`, `wobble_mm`,
  `max_drift_mm` (e.g. 0.5, 0.4, 4.0).
- `--seed N` makes a page reproducible; change it for a different-looking copy.
- `--no-jitter` / `--deform` force jitter off / deformation on.
