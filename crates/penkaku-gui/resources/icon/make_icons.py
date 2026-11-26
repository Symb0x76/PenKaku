"""Generate the app icons in this folder from the master artwork.

    uv run --with pillow python crates/penkaku-gui/resources/icon/make_icons.py docs/OriginIcon.png

The master (docs/OriginIcon.png) is not in git; the generated files are:
- penkaku.ico   Windows exe and window icon (16-256 px), embedded by build.rs
- penkaku.icns  macOS app bundle icon (16-1024 px), copied into the .app by CI
- png/<n>.png     Linux hicolor icons; 256.png is also the X11 window icon (main.rs)
"""

import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
LINUX_SIZES = [16, 32, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def square(master: Image.Image, margin: float = 0.04) -> Image.Image:
    """Crop to the drawn area, then centre it on a transparent square with a small margin."""
    art = master.convert("RGBA")
    art = art.crop(art.getbbox())
    side = round(max(art.size) * (1 + 2 * margin))
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(art, ((side - art.width) // 2, (side - art.height) // 2))
    return canvas.resize((1024, 1024), Image.LANCZOS)


def main() -> None:
    icon = square(Image.open(sys.argv[1]))
    (HERE / "png").mkdir(exist_ok=True)
    for n in LINUX_SIZES:
        icon.resize((n, n), Image.LANCZOS).save(HERE / "png" / f"{n}.png", optimize=True)
    icon.save(HERE / "penkaku.ico", sizes=[(n, n) for n in ICO_SIZES])
    icon.save(HERE / "penkaku.icns")


if __name__ == "__main__":
    main()
