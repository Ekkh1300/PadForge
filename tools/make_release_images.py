"""Produce the release assets: a PNG of the icon at a few sizes, and a square
icon for the GitHub release page.

GitHub does not render .ico on a release page, and it rounds any non-square image
to a circle, so both problems are solved by handing it square PNGs. The 512px one
becomes the repository avatar, where GitHub crops to a circle itself.
"""
import os
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "tools"))

from make_icon import draw_pad  # noqa: E402

DOCS = os.path.join(HERE, "..", "docs")


def main():
    os.makedirs(DOCS, exist_ok=True)

    written = []

    # The GitHub avatar and release header.
    for size in (128, 512):
        path = os.path.join(DOCS, f"icon-{size}.png")
        draw_pad(size).save(path, "PNG")
        written.append(path)

    # A strip of the real sizes on a mid-grey background, which is roughly what a
    # title bar looks like, so the mark can be judged at the size it is mostly
    # seen at rather than only blown up.
    sizes = [16, 20, 24, 32, 48, 64]
    pad = 16
    cell = 64 + pad
    sheet = Image.new("RGBA", (cell * len(sizes) + pad, 128 + pad * 2), (0x60, 0x64, 0x6C, 255))

    from PIL import ImageDraw

    d = ImageDraw.Draw(sheet)
    for i, size in enumerate(sizes):
        icon = draw_pad(size)
        x = pad + i * cell
        # At real size, on its own row.
        sheet.alpha_composite(icon, (x, pad))
        # And enlarged, nearest-neighbour, so the pixels can be inspected.
        big = icon.resize((64, 64), Image.NEAREST)
        sheet.alpha_composite(big, (x, pad + 70))
        d.text((x, pad + 62), f"{size}", fill=(0xFF, 0xFF, 0xFF, 255))

    strip = os.path.join(DOCS, "icon-sizes.png")
    sheet.convert("RGB").save(strip, "PNG")
    written.append(strip)

    for path in written:
        size = os.path.getsize(path)
        print(f"  wrote {os.path.relpath(path, DOCS)}  ({size:,} bytes)")


if __name__ == "__main__":
    main()