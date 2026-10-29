#!/usr/bin/env python3
"""Renders apps/mareader/icons from logo.svg.

The .icns keeps Apple's 824-on-1024 grid with a soft shadow under the
tile; the PNGs and the .ico crop to the tile, as Windows and Linux draw
icons edge to edge. Needs cairosvg and Pillow:

    pip install cairosvg pillow
    python3 tools/icons.py
"""

import io
from pathlib import Path

import cairosvg
from PIL import Image, ImageFilter

ICONS = Path(__file__).resolve().parent.parent / "apps/mareader/icons"
# The tile's box on the 1024 canvas.
TILE = (100, 100, 924, 924)


def render(side: int) -> Image.Image:
    """The whole 1024 canvas at `side` pixels a side."""
    png = cairosvg.svg2png(
        url=str(ICONS / "logo.svg"), output_width=side, output_height=side
    )
    return Image.open(io.BytesIO(png)).convert("RGBA")


def tile(side: int) -> Image.Image:
    """The tile alone, edge to edge, rendered large then scaled down."""
    big = render(2048).crop(tuple(v * 2 for v in TILE))
    return big.resize((side, side), Image.LANCZOS)


def mac(side: int) -> Image.Image:
    """The canvas with the shadow macOS icons cast, as Apple's grid has."""
    art = render(side)
    alpha = art.getchannel("A")
    k = side / 1024
    shadow = Image.new("RGBA", art.size, (0, 0, 0, 0))
    shade = alpha.point(lambda a: int(a * 0.32))
    shadow.putalpha(shade.filter(ImageFilter.GaussianBlur(14 * k)))
    out = Image.new("RGBA", art.size, (0, 0, 0, 0))
    out.alpha_composite(shadow, (0, round(10 * k)))
    out.alpha_composite(art)
    return out


def main() -> None:
    for name, side in [("32x32", 32), ("128x128", 128), ("128x128@2x", 256)]:
        tile(side).save(ICONS / f"{name}.png", optimize=True)
    # Each size drawn on its own, so small ones stay sharp.
    sizes = [16, 24, 32, 48, 64, 128, 256]
    small = [tile(s) for s in sizes[:-1]]
    tile(256).save(ICONS / "icon.ico", sizes=[(s, s) for s in sizes], append_images=small)
    mac_sizes = [16, 32, 64, 128, 256, 512]
    mac(1024).save(ICONS / "icon.icns", append_images=[mac(s) for s in mac_sizes])


if __name__ == "__main__":
    main()
