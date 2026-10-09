"""Generates PadForge's application icon.

The mark is a DualShock 4 pad seen from the front, drawn small: at 16 px in a
title bar almost nothing survives, so the silhouette has to do the work. The
body is a rounded rectangle, the touchpad a lighter inset, and the sticks and
buttons just enough to read as a controller rather than a plain tile.

The .ico is assembled here rather than left to Pillow, because Pillow's ICO
writer does not reliably emit every requested size into the directory: it can
produce a file whose offsets point far past its own end. Writing the container
by hand makes the eight frames and their offsets verifiable.

Run:  python make_icon.py
"""

import math
import os
import struct
import zlib

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "assets", "padforge.ico")

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


def draw_pad(size: int) -> Image.Image:
    """Draw the pad at `size` px square, supersampled then downscaled."""
    ss = 4
    s = size * ss
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))

    def u(v: float) -> float:
        """A proportion of the canvas, in pixels, so shapes scale together."""
        return v * s

    # --- body -------------------------------------------------------------
    # A controller is wider than tall, so the canvas keeps a margin on the sides.
    pad_w = u(0.86)
    pad_h = u(0.94)
    left = (s - pad_w) / 2
    top = (s - pad_h) / 2
    radius = pad_w * 0.22
    line = max(1, int(ss * 0.9))

    # Build the body on its own so the gradient can be clipped to its rounded
    # shape, then composite it through that shape as a mask.
    body = Image.new("RGBA", (int(pad_w) + 2, int(pad_h) + 2), (0, 0, 0, 0))
    bd = ImageDraw.Draw(body)
    bw, bh = body.size
    bd.rounded_rectangle((0, 0, bw - 1, bh - 1), radius=radius, fill=BODY_BOTTOM + (255,))

    grad = Image.new("RGBA", body.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(grad)
    for y in range(bh):
        t = y / max(1, bh - 1)
        gd.line(
            [(0, y), (bw, y)],
            fill=tuple(
                int(BODY_TOP[i] + (BODY_BOTTOM[i] - BODY_TOP[i]) * t) for i in range(3)
            )
            + (255,),
        )
    body = Image.alpha_composite(body, grad)

    # Outline last, so it sits over the gradient rather than under it.
    ImageDraw.Draw(body).rounded_rectangle(
        (0, 0, bw - 1, bh - 1), radius=radius, outline=EDGE + (255,), width=line
    )

    mask = Image.new("L", body.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, bw - 1, bh - 1), radius=radius, fill=255)
    img.paste(body, (int(left), int(top)), mask)

    d = ImageDraw.Draw(img)

    # --- touchpad ---------------------------------------------------------
    tp_w = pad_w * 0.40
    tp_h = pad_h * 0.34
    tp_left = left + (pad_w - tp_w) / 2
    tp_top = top + pad_h * 0.075
    d.rounded_rectangle(
        (tp_left, tp_top, tp_left + tp_w, tp_top + tp_h),
        radius=tp_w * 0.14,
        fill=TOUCHPAD + (255,),
        outline=TOUCHPAD_EDGE + (255,),
        width=max(1, int(ss * 0.7)),
    )

    # --- lightbar ---------------------------------------------------------
    # The one saturated element, where the real one sits: below the touchpad.
    bar_w = pad_w * 0.30
    bar_h = max(ss * 1.2, pad_h * 0.035)
    bar_left = left + (pad_w - bar_w) / 2
    bar_top = top + pad_h * 0.505
    d.rounded_rectangle(
        (bar_left, bar_top, bar_left + bar_w, bar_top + bar_h),
        radius=bar_h / 2,
        fill=LIGHTBAR + (255,),
    )

    # --- sticks -----------------------------------------------------------
    stick_r = pad_w * 0.085
    stick_cy = top + pad_h * 0.655
    for cx in (left + pad_w * 0.29, left + pad_w * 0.71):
        d.ellipse(
            (cx - stick_r, stick_cy - stick_r, cx + stick_r, stick_cy + stick_r),
            fill=STICK + (255,),
            outline=STICK_RING + (255,),
            width=max(1, int(ss * 0.7)),
        )

    # --- face buttons -----------------------------------------------------
    # Four dots in a diamond. Below about 32 px four separate circles turn to
    # mush, so at small sizes a single bar stands in for the whole cluster: it
    # still reads as "buttons here" and survives the downscale.
    if size >= 32:
        face_r = pad_w * 0.052
        fcx = left + pad_w * 0.855
        for dx, dy in ((0, -1), (0, 1), (-1, 0), (1, 0)):
            bx, by = fcx + dx * face_r * 2.0, stick_cy + dy * face_r * 2.0
            d.ellipse(
                (bx - face_r, by - face_r, bx + face_r, by + face_r), fill=D_PAD + (255,)
            )
    else:
        bw_ = pad_w * 0.10
        bh_ = pad_h * 0.13
        bx_ = left + pad_w * 0.80
        d.rounded_rectangle(
            (bx_ - bw_, stick_cy - bh_, bx_ + bw_, stick_cy + bh_),
            radius=bw_ * 0.4,
            fill=D_PAD + (255,),
        )

    return img.resize((size, size), Image.LANCZOS)


def png_bytes(img: Image.Image) -> bytes:
    """Encode as PNG.

    Not used by default. PNG-compressed frames are legal inside an .ico from
    Windows Vista on and are much smaller, but windres mis-parses them when it
    reads the .ico for an RC file: it rewrites each frame as a raw DIB and the
    group directory ends up pointing at the wrong bytes, which shows up as a
    corrupt icon rather than as an error.
    """
    import io

    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def bmp_bytes(img: Image.Image) -> bytes:
    """Encode as a 32bpp BGRA DIB, with the AND mask windres expects.

    This is the only encoding used, because it is the only one windres handles
    correctly. Every frame therefore carries both an XOR colour bitmap and an
    alpha mask.
    """
    w, h = img.size
    pixels = img.load()

    # BITMAPINFOHEADER. biHeight is doubled because the XOR and AND masks are
    # stored stacked, which is what makes this a valid .ico frame rather than a
    # plain DIB.
    header = struct.pack(
        "<IiiHHIIiiII",
        40,     # biSize
        w,      # biWidth
        h * 2,  # biHeight: XOR followed by AND
        1,      # biPlanes
        32,     # biBitCount
        0,      # biCompression: BI_RGB
        0,      # biSizeImage
        0, 0, 0, 0,
    )

    # XOR mask, bottom-up BGRA. The alpha channel is what modern Windows uses to
    # draw a rounded, antialiased edge, so it is kept as-is.
    xor = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = pixels[x, y]
            xor += bytes((b, g, r, a))

    # AND mask: one bit per pixel, each row padded to a 4-byte boundary. Fully
    # opaque, because the alpha channel already carries the shape.
    row_bytes = ((w + 31) // 32) * 4
    and_mask = bytes(row_bytes * h)

    return header + bytes(xor) + and_mask


def build_ico(frames: list[Image.Image]) -> bytes:
    """Assemble a multi-image .ico with a correct directory."""
    entries = []
    payloads = []
    for img in frames:
        data = bmp_bytes(img)
        entries.append((img.size[0], img.size[1], data))
        payloads.append(data)

    count = len(entries)
    header = struct.pack("<HHH", 0, 1, count)

    # Each ICONDIRENTRY is exactly 16 bytes, and the data starts after all of
    # them. The size is asserted so a wrong layout cannot pass silently.
    entry_size = struct.calcsize("<BBBBHHII")
    assert entry_size == 16, f"ICONDIRENTRY must be 16 bytes, got {entry_size}"

    offset = 6 + 16 * count
    directory = bytearray()
    for (w, h, data) in entries:
        directory += struct.pack(
            "<BBBBHHII",
            w if w < 256 else 0,
            h if h < 256 else 0,
            0,      # colour count: 0 means "no palette"
            0,      # reserved
            1,      # colour planes
            32,     # bits per pixel
            len(data),
            offset,
        )
        offset += len(data)

    return header + bytes(directory) + b"".join(payloads)


def verify(data: bytes, expected: list[int]) -> list[str]:
    """Parse the file back and confirm every frame is where it claims to be."""
    problems = []
    reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if kind != 1:
        problems.append(f"type is {kind}, expected 1 (icon)")
    if count != len(expected):
        problems.append(f"{count} images, expected {len(expected)}")

    for i in range(count):
        w, h, _c, _r, _p, bits, size, off = struct.unpack_from(
            "<BBBBHHII", data, 6 + i * 16
        )
        dim = (w or 256, h or 256)
        if dim != (expected[i], expected[i]):
            problems.append(f"[{i}] is {dim}, expected {expected[i]}")
        if bits != 32:
            problems.append(f"[{i}] is {bits}bpp, expected 32")
        if off + size > len(data):
            problems.append(f"[{i}] claims bytes {off}..{off + size} of a {len(data)} byte file")
        print(f"    [{i}] {dim[0]:>3}x{dim[1]:<3} {bits}bpp, {size:>6} bytes at {off}")
    return problems


def main() -> None:
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
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