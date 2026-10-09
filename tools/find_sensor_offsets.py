"""Locate the gyro and accelerometer inside a captured frame, by search.

Rather than trusting a port's offsets, this finds where a physically plausible
reading lives. A DS4 lying still must show about 1 g on the accelerometer, and
near zero on the gyro. That is a property of the hardware, so any offset that
produces it is the right one, and any offset that does not is wrong.
"""

import math

HEX = (
    "11 c0 00 80 80 80 80 08 00 00 00 00 00 4b 10 07 00 f0 ff 16 00 c0 fe 1f 1f 79 05 "
    "00 00 00 00 00 01 00 00 01 00 80 00 00 00 80 00 00 00 00 80 00 00 00 80 00 00 00 "
    "00 80 00 00 00 80 00 00 00 00 80 00 00 00 80 00 00 00 00 80 00 00 00 00 00 a2 21 "
    "9e 34"
)

buf = [int(x, 16) for x in HEX.split()]
end = len(buf)
while end > 1 and buf[end - 1] == 0:
    end -= 1


def s16(a, b):
    v = a | (b << 8)
    return v - 0x10000 if v & 0x8000 else v


def accel_at(off):
    return [s16(buf[off + i], buf[off + i + 1]) / 8192.0 for i in (0, 2, 4)]


def gyro_at(off):
    return [s16(buf[off + i], buf[off + i + 1]) / 16.0 for i in (0, 2, 4)]


print("every offset where the accelerometer reads a plausible 1 g")
print(f"{'offset':>7}  {'x':>8} {'y':>8} {'z':>8}  {'|a|':>7}")
hits = []
for off in range(0, end - 6):
    a = accel_at(off)
    mag = math.sqrt(sum(v * v for v in a))
    if 0.85 < mag < 1.15:
        hits.append((off, a, mag))
        print(f"{off:>7}  {a[0]:>8.3f} {a[1]:>8.3f} {a[2]:>8.3f}  {mag:>7.3f}")

print(f"\n{len(hits)} offset(s) qualify")
if hits:
    # The accelerometer sits above the gyro in the DS4 report, and the gyro is
    # three 16-bit values. Whichever hit has a plausible gyro six bytes earlier
    # is the pair.
    for off, a, mag in hits:
        g_off = off - 6
        if g_off < 0:
            continue
        g = gyro_at(g_off)
        print(f"\ncandidate: gyro at {g_off}, accel at {off}")
        print(f"  gyro  ({g[0]:.1f}, {g[1]:.1f}, {g[2]:.1f}) deg/s")
        print(f"  accel ({a[0]:.3f}, {a[1]:.3f}, {a[2]:.3f}) g, |a| = {mag:.3f} g")

print("\nfor comparison, the offset the port guessed")
a = accel_at(26)
print(f"  accel at 26: ({a[0]:.3f}, {a[1]:.3f}, {a[2]:.3f}) g, |a| = {math.sqrt(sum(v*v for v in a)):.3f} g")
print("  a pad at rest cannot read 0 g, so this is simply the wrong place")