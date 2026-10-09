"""Generates PadForge's application icon.

The mark is a DualShock 4 pad seen from the front, drawn small: at 16 px in a
title bar almost nothing survives, so the silhouette has to do the work. The
body is a rounded rectangle, the touchpad a lighter inset, and the sticks and
buttons just enough to read as a controller rather than a plain tile.

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
BODY_TOP = (58, 66, 82)
BODY_BOTTOM = (34, 39, 50)
EDGE = (86, 97, 120)
TOUCHPAD = (48, 55, 70)
TOUCHPAD_EDGE = (72, 82, 102)
STICK = (24, 28, 36)
STICK_RING = (108, 122, 148)
LIGHTBAR = (86, 182, 255)
D_PAD = (74, 84, 104)

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

    def stroke_round(self, x0, y0, x1, y1, colour, radius, width):
        """Draw a rounded outline by filling the shape and punching out the
        interior. Punching rather than tracing keeps this to one primitive."""
        outer = Canvas(self.w, self.h)
        outer.fill_rect(x0, y0, x1, y1, colour, radius=radius)

        inner = Canvas(self.w, self.h)
        ix0, iy0 = x0 + width, y0 + width
        ix1, iy1 = x1 - width, y1 - width
        if ix1 > ix0 and iy1 > iy0:
            inner.fill_rect(ix0, iy0, ix1, iy1, (0, 0, 0), radius=max(0, radius - width))

        for i in range(self.w * self.h):
            o = i * 4
            if outer.px[o + 3] and not inner.px[o + 3]:
                self.blend(i % self.w, i // self.w, outer.px[o : o + 3])

    def fill_ellipse(self, cx, cy, rx, ry, colour):
        for y in range(max(0, int(cy - ry)), min(self.h, int(cy + ry) + 1)):
            for x in range(max(0, int(cx - rx)), min(self.w, int(cx + rx) + 1)):
                dx = (x + 0.5 - cx) / rx
                dy = (y + 0.5 - cy) / ry
                if dx * dx + dy * dy <= 1.0:
                    self.blend(x, y, colour)

    def stroke_ellipse(self, cx, cy, rx, ry, colour, width):
        outer = Canvas(self.w, self.h)
        outer.fill_ellipse(cx, cy, rx, ry, colour)
        inner = Canvas(self.w, self.h)
        inner.fill_ellipse(cx, cy, max(0.1, rx - width), max(0.1, ry - width), (0, 0, 0))
        for i in range(self.w * self.h):
            o = i * 4
            if outer.px[o + 3] and not inner.px[o + 3]:
                self.blend(i % self.w, i // self.w, outer.px[o : o + 3])

    def line(self, x0, y0, x1, y1, colour, width=1):
        """A thick line, drawn by stamping a square brush along the span."""
        steps = int(max(abs(x1 - x0), abs(y1 - y0)) * 2) + 1
        for s in range(steps + 1):
            t = s / steps
            x = x0 + (x1 - x0) * t
            y = y0 + (y1 - y0) * t
            half = width / 2
            self.fill_rect(x - half, y - half, x + half + 1, y + half + 1, colour)

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


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def draw_pad(size):
    """Draw the mark at `size` px square, supersampled then downsampled."""
    s = size * SS
    c = Canvas(s, s)

    def u(v):
        return v * s

    # A controller is wider than tall, so the canvas keeps a margin on the sides.
    pad_w = u(0.86)
    pad_h = u(0.94)
    left = (s - pad_w) / 2
    top = (s - pad_h) / 2
    radius = pad_w * 0.22

    # Body, with a vertical gradient so the shell reads as lit from above rather
    # than as a flat grey blob at every size.
    grad = Canvas(s, s)
    for y in range(int(top), int(top + pad_h) + 1):
        t = (y - top) / pad_h
        grad.fill_rect(left, y, left + pad_w, y + 1, lerp(BODY_TOP, BODY_BOTTOM, t))
    # Clip the gradient to the rounded silhouette.
    mask = Canvas(s, s)
    mask.fill_rect(left, top, left + pad_w, top + pad_h, (255, 255, 255), radius=radius)
    for i in range(s * s):
        if not mask.px[i * 4 + 3]:
            grad.px[i * 4 + 3] = 0
    c.px[:] = grad.px

    c.stroke_round(left, top, left + pad_w, top + pad_h, EDGE, radius, SS * 0.9)

    # Touchpad.
    tp_w = pad_w * 0.40
    tp_h = pad_h * 0.34
    tp_left = left + (pad_w - tp_w) / 2
    tp_top = top + pad_h * 0.075
    c.fill_rect(tp_left, tp_top, tp_left + tp_w, tp_top + tp_h, TOUCHPAD, radius=tp_w * 0.14)
    c.stroke_round(tp_left, tp_top, tp_left + tp_w, tp_top + tp_h, TOUCHPAD_EDGE, tp_w * 0.14, SS * 0.7)

    # Lightbar: the one saturated element, where the real one sits.
    bar_w = pad_w * 0.30
    bar_h = max(SS * 1.2, pad_h * 0.035)
    bar_left = left + (pad_w - bar_w) / 2
    bar_top = top + pad_h * 0.505
    c.fill_rect(bar_left, bar_top, bar_left + bar_w, bar_top + bar_h, LIGHTBAR, radius=bar_h / 2)

    # Sticks.
    stick_r = pad_w * 0.085
    stick_cy = top + pad_h * 0.655
    for cx in (left + pad_w * 0.29, left + pad_w * 0.71):
        c.fill_ellipse(cx, stick_cy, stick_r, stick_r, STICK)
        c.stroke_ellipse(cx, stick_cy, stick_r, stick_r, STICK_RING, SS * 0.7)

    # Face buttons. Below 32 px four separate circles turn to mush, so a single
    # bar stands in: it still reads as "buttons here" and survives the downscale.
    if size >= 32:
        face_r = pad_w * 0.052
        fcx = left + pad_w * 0.855
        for dx, dy in ((0, -1), (0, 1), (-1, 0), (1, 0)):
            c.fill_ellipse(fcx + dx * face_r * 2.0, stick_cy + dy * face_r * 2.0, face_r, face_r, D_PAD)
    else:
        bw = pad_w * 0.10
        bh = pad_h * 0.13
        bx = left + pad_w * 0.80
        c.fill_rect(bx - bw, stick_cy - bh, bx + bw, stick_cy + bh, D_PAD, radius=bw * 0.4)

    return c.downsample(SS)


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
    frames = [draw_pad(s) for s in SIZES]
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