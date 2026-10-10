"""Generates PadForge's application icon.

The mark is the one the app draws for its own window: two rings on a dark
rounded square. It is generated here rather than shipped as an image so that the
artwork is not checked in as an opaque blob nobody can edit, and so that the
same shapes appear on the title bar, the tray, the desktop shortcut, the Start
Menu, the installer and the file's properties dialog.

Keeping it to two rings is a decision about size, not about taste. At 16 px a
detailed drawing loses its details and keeps only its noise, while two
concentric circles and a corner radius still read as a deliberate shape. That is
why the mark is drawn with primitives rather than as a small illustration.

Standard library only. This runs in CI to prove the committed .ico matches, and
requiring Pillow there would mean either installing it on every runner or
trusting a resource the build depends on without being able to check it. A
64x64 supersampled canvas, a rounding routine and a hand-built ICO directory is
not much code for that independence.

Run:  python make_icon.py
"""

import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "padforge.ico")

# Windows asks for these; it picks the closest match per surface rather than
# scaling one bitmap.
SIZES = [16, 20, 24, 32, 48, 64, 128, 256]

# The application's own palette, so the icon and the window agree.
#
# These are the exact values `icons.rs` paints at runtime, and the accent is the
# same one `theme.rs` uses for every highlighted control. If the two ever drift,
# the icon and the window stop looking like the same program, which is the bug
# this file exists to prevent.
SQUARE = (0x12, 0x15, 0x1B)
ACCENT = (0x35, 0xD0, 0xE8)

# Geometry, as fractions of the canvas. These match the runtime mark exactly:
# the outer ring guided by a circle at 0.30 of the size, the inner at 0.14, each
# 0.055 of the size thick, on a square inset by 0.04 with a 0.22 corner radius.
SQUARE_INSET = 0.04
CORNER_RADIUS = 0.22
RING_OUTER = 0.30
RING_INNER = 0.14
RING_THICKNESS = 0.055

SS = 4  # supersample factor


class Canvas:
    """A very small RGBA raster with the few primitives the mark needs."""

    def __init__(self, w, h):
        self.w = w
        self.h = h
        # Row-major, four bytes per pixel, straight RGBA.
        self.px = bytearray(w * h * 4)

    def blend(self, x, y, colour):
        """Source-over composite of one pixel. Alpha is kept, not replaced."""
        if x < 0 or y < 0 or x >= self.w or y >= self.h:
            return
        r, g, b = colour
        i = (y * self.w + x) * 4
        a_src = 255
        a_dst = self.px[i + 3]
        if a_dst == 0:
            self.px[i : i + 4] = bytes((r, g, b, a_src))
            return
        # Both source and destination are opaque, so this is a plain lerp.
        for k, v in enumerate((r, g, b)):
            self.px[i + k] = (v * a_src + self.px[i + k] * a_dst) // (a_src + a_dst)

    def fill_rect(self, x0, y0, x1, y1, colour, radius=0):
        """Fill an axis-aligned rect, optionally with rounded corners."""
        x0, y0 = max(0, int(x0)), max(0, int(y0))
        x1, y1 = min(self.w, int(x1)), min(self.h, int(y1))
        for y in range(y0, y1):
            for x in range(x0, x1):
                if radius and not self._inside_round(x + 0.5, y + 0.5, x0, y0, x1, y1, radius):
                    continue
                self.blend(x, y, colour)

    def fill_ellipse(self, cx, cy, rx, ry, colour):
        for y in range(max(0, int(cy - ry)), min(self.h, int(cy + ry) + 1)):
            for x in range(max(0, int(cx - rx)), min(self.w, int(cx + rx) + 1)):
                dx = (x + 0.5 - cx) / rx
                dy = (y + 0.5 - cy) / ry
                if dx * dx + dy * dy <= 1.0:
                    self.blend(x, y, colour)

    def _inside_round(self, px, py, x0, y0, x1, y1, radius):
        """Point-in-rounded-rectangle test, with the corners treated as circles."""
        cx = min(max(px, x0 + radius), x1 - radius)
        cy = min(max(py, y0 + radius), y1 - radius)
        dx = px - cx
        dy = py - cy
        return dx * dx + dy * dy <= radius * radius

    def downsample(self, factor):
        """Box-filter down to factor-times smaller, which is what gives the edges
        their antialiasing."""
        w = self.w // factor
        h = self.h // factor
        out = Canvas(w, h)
        area = factor * factor
        for y in range(h):
            for x in range(w):
                r = g = b = a = 0
                for dy in range(factor):
                    row = (y * factor + dy) * self.w
                    for dx in range(factor):
                        i = (row + x * factor + dx) * 4
                        r += self.px[i]
                        g += self.px[i + 1]
                        b += self.px[i + 2]
                        a += self.px[i + 3]
                o = (y * w + x) * 4
                out.px[o] = r // area
                out.px[o + 1] = g // area
                out.px[o + 2] = b // area
                out.px[o + 3] = a // area
        return out

    def bgra_rows_bottom_up(self):
        """32bpp BGRA, bottom row first, which is what a DIB inside an .ico wants."""
        out = bytearray()
        stride = self.w * 4
        for y in range(self.h - 1, -1, -1):
            row = y * stride
            for x in range(self.w):
                i = row + x * 4
                out += bytes((self.px[i + 2], self.px[i + 1], self.px[i], self.px[i + 3]))
        return bytes(out)


def draw_mark(size):
    """Draw the mark at `size` px square, supersampled then downsampled.

    The supersampling matters most here: without it a ring 3 px wide grows a
    visible stair-step at the diagonal, and the corner radius of the square ends
    up as a single chipped pixel at 16 px.
    """
    s = size * SS
    c = Canvas(s, s)
    inset = s * SQUARE_INSET
    low, high = inset, s - inset
    radius = s * CORNER_RADIUS

    for y in range(s):
        for x in range(s):
            if inside_round(x + 0.5, y + 0.5, low, low, high, high, radius):
                c.blend(x, y, SQUARE)

    # Two rings, drawn as a filled disc minus a smaller one. Painting the disc
    # and then erasing its middle keeps this to the two primitives the canvas
    # already has. The stroke is centred on the radius, exactly as the window
    # measures it (`|d - radius| <= thickness / 2`), so a ring sits half its
    # width either side of the guide circle instead of hanging inside it.
    cx = cy = s / 2.0
    thickness = max(s * RING_THICKNESS, SS)
    for fraction in (RING_OUTER, RING_INNER):
        radius_px = s * fraction
        outer = Canvas(s, s)
        outer.fill_ellipse(cx, cy, radius_px + thickness / 2, radius_px + thickness / 2, ACCENT)
        inner = Canvas(s, s)
        inner.fill_ellipse(
            cx,
            cy,
            max(0.1, radius_px - thickness / 2),
            max(0.1, radius_px - thickness / 2),
            (0, 0, 0),
        )
        for i in range(s * s):
            o = i * 4
            # Only where the square already painted, so a ring never spills past
            # the corner radius -- and written straight through rather than
            # composited. Compositing averages the ring with the square beneath
            # it, which lands on a dull teal instead of the accent, and a second
            # pass halves it again. The window draws the accent at full strength,
            # so anything less here is a different icon wearing the same shapes.
            if outer.px[o + 3] and not inner.px[o + 3] and c.px[o + 3]:
                c.px[o : o + 3] = outer.px[o : o + 3]

    return c.downsample(SS)


def inside_round(px, py, x0, y0, x1, y1, radius):
    """Point-in-rounded-rectangle test, with the corners treated as circles."""
    cx = min(max(px, x0 + radius), x1 - radius)
    cy = min(max(py, y0 + radius), y1 - radius)
    dx = px - cx
    dy = py - cy
    return dx * dx + dy * dy <= radius * radius


def bmp_bytes(img):
    """A 32bpp BGRA DIB with its own mask.

    Every frame is a raw DIB, not a PNG. PNG is legal inside an .ico and much
    smaller, but windres rewrites each frame as a DIB when it reads an .ico for an
    RC file, and the group directory then points at the wrong bytes. That shows up
    as a corrupt icon rather than as an error.
    """
    w, h = img.w, img.h
    header = struct.pack(
        "<IiiHHIIiiII",
        40,     # biSize
        w,      # biWidth
        h * 2,  # biHeight: the XOR and AND masks are stacked
        1,      # biPlanes
        32,     # biBitCount
        0,      # BI_RGB
        0,      # biSizeImage
        0, 0, 0, 0,
    )
    xor = img.bgra_rows_bottom_up()
    # AND mask: one bit per pixel, rows padded to 4 bytes. Fully opaque, because
    # the alpha channel already carries the shape.
    row_bytes = ((w + 31) // 32) * 4
    return header + xor + bytes(row_bytes * h)


def build_ico(frames):
    """Assemble a multi-image .ico with a directory that is verified below."""
    count = len(frames)
    header = struct.pack("<HHH", 0, 1, count)

    entry_size = struct.calcsize("<BBBBHHII")
    assert entry_size == 16, f"ICONDIRENTRY must be 16 bytes, got {entry_size}"

    payloads = [bmp_bytes(f) for f in frames]
    offset = 6 + 16 * count
    directory = bytearray()
    for (frame, data) in zip(frames, payloads):
        w, h = frame.w, frame.h
        directory += struct.pack(
            "<BBBBHHII",
            w if w < 256 else 0,   # a stored 0 means 256
            h if h < 256 else 0,
            0,                      # no palette
            0,                      # reserved
            1,                      # colour planes
            32,                     # bits per pixel
            len(data),
            offset,
        )
        offset += len(data)

    return header + bytes(directory) + b"".join(payloads)


def verify(data, expected):
    """Parse the file back and confirm every frame is where it claims to be."""
    problems = []
    _reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if kind != 1:
        problems.append(f"type is {kind}, expected 1 (icon)")
    if count != len(expected):
        problems.append(f"{count} images, expected {len(expected)}")

    for i in range(count):
        w, h, _c, _r, _p, bits, size, off = struct.unpack_from("<BBBBHHII", data, 6 + i * 16)
        dim = (w or 256, h or 256)
        if dim != (expected[i], expected[i]):
            problems.append(f"[{i}] is {dim}, expected {expected[i]}")
        if bits != 32:
            problems.append(f"[{i}] is {bits}bpp, expected 32")
        if off + size > len(data):
            problems.append(f"[{i}] claims bytes {off}..{off + size} of a {len(data)} byte file")
        print(f"    [{i}] {dim[0]:>3}x{dim[1]:<3} {bits}bpp, {size:>6} bytes at {off}")
    return problems


def main():
    frames = [draw_mark(s) for s in SIZES]
    data = build_ico(frames)

    print(f"wrote {OUT} ({len(data)} bytes)")
    problems = verify(data, SIZES)
    if problems:
        for p in problems:
            print(f"  PROBLEM: {p}")
        raise SystemExit(1)

    with open(OUT, "wb") as fh:
        fh.write(data)
    print("  verified: directory, offsets and sizes all consistent")


if __name__ == "__main__":
    main()