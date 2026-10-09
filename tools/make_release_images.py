"""Produce the release assets: a PNG of the icon at a few sizes.

GitHub does not render .ico on a release page, and it rounds any non-square image
to a circle, so both problems are solved by handing it square PNGs. The 512px one
becomes the repository avatar.

Standard library only, like make_icon.py, so this runs anywhere the build does.

Run:  python make_release_images.py
"""
import os
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from make_icon import SS, Canvas, draw_pad  # noqa: E402

DOCS = os.path.join(HERE, "..", "docs")


def write_png(path, width, height, rgba):
    """Minimal PNG writer: 8-bit RGBA, one IDAT, filter type 0."""
    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)
        raw += rgba[y * stride : (y + 1) * stride]

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as fh:
        fh.write(png)


def to_rgba(img):
    """The canvas's straight RGBA bytes, top row first, as PNG wants them."""
    out = bytearray()
    for y in range(img.h):
        out += img.px[y * img.w * 4 : (y + 1) * img.w * 4]
    return bytes(out)


def smooth_resize(img, target):
    """Box-filter upscale, for showing the small sizes enlarged.

    Nearest-neighbour was tried first and is wrong here: it turns the rounded
    corners into visible stair steps, which makes the mark look worse than it is.
    Averaging the source pixels that map into each output pixel keeps the edge a
    soft edge, which is what it actually is.
    """
    out = Canvas(target, target)
    for y in range(target):
        for x in range(target):
            # The source span this output pixel covers.
            sx0 = x * img.w // target
            sx1 = max(sx0 + 1, (x + 1) * img.w // target)
            sy0 = y * img.h // target
            sy1 = max(sy0 + 1, (y + 1) * img.h // target)
            r = g = b = a = 0
            n = 0
            for sy in range(sy0, min(sy1, img.h)):
                for sx in range(sx0, min(sx1, img.w)):
                    i = (sy * img.w + sx) * 4
                    r += img.px[i]
                    g += img.px[i + 1]
                    b += img.px[i + 2]
                    a += img.px[i + 3]
                    n += 1
            if n == 0:
                continue
            o = (y * target + x) * 4
            out.px[o] = r // n
            out.px[o + 1] = g // n
            out.px[o + 2] = b // n
            out.px[o + 3] = a // n
    return out


def composite(dst, src, x, y):
    """Draw `src` onto `dst` at an offset.

    Only the pixels where `src` is opaque are copied. Iterating over the source
    rather than the destination matters: the destination is far larger and mostly
    background, and walking it would repaint the background over itself.
    """
    for sy in range(src.h):
        for sx in range(src.w):
            i = (sy * src.w + sx) * 4
            if not src.px[i + 3]:
                continue
            dx, dy = x + sx, y + sy
            if dx < 0 or dy < 0 or dx >= dst.w or dy >= dst.h:
                continue
            o = (dy * dst.w + dx) * 4
            dst.px[o : o + 4] = src.px[i : i + 4]


def main():
    os.makedirs(DOCS, exist_ok=True)
    written = []

    # The GitHub avatar and release header.
    for size in (128, 512):
        img = draw_pad(size)
        path = os.path.join(DOCS, f"icon-{size}.png")
        write_png(path, size, size, to_rgba(img))
        written.append(path)

    # A strip of the real sizes on a mid-grey background, which is roughly what a
    # title bar looks like, so the mark can be judged at the size it is mostly
    # seen at rather than only blown up.
    sizes = [16, 20, 24, 32, 48, 64]
    pad = 16
    cell = 80
    w = cell * len(sizes) + pad * 2
    h = 132
    sheet = Canvas(w, h)
    # Background, mid grey, opaque.
    for y in range(h):
        for x in range(w):
            i = (y * w + x) * 4
            sheet.px[i : i + 4] = bytes((0x60, 0x64, 0x6C, 255))

    for i, size in enumerate(sizes):
        icon = draw_pad(size)
        x = pad + i * cell
        # Top row: at real size, which is what Windows actually draws.
        composite(sheet, icon, x, pad)
        # Bottom row: enlarged with a box filter, to show the mark's detail. The
        # stair-stepping is genuine and not a rendering artefact: at 16 px the
        # rounded corner really is that coarse, which is the whole reason the mark
        # leans on its silhouette rather than on detail.
        composite(sheet, smooth_resize(icon, 64), x, pad + 52)

    strip = os.path.join(DOCS, "icon-sizes.png")
    write_png(strip, w, h, to_rgba(sheet))
    written.append(strip)

    for path in written:
        print(f"  wrote {os.path.relpath(path, DOCS)}  ({os.path.getsize(path):,} bytes)")


if __name__ == "__main__":
    main()