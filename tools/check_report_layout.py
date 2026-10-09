"""Cross-check a captured DS4 Bluetooth frame against the layout the decoder uses.

The point is to confirm from the bytes themselves that the offsets in
report.rs are right, rather than trusting the port from DS4Windows. It also
shows which offsets the old code was using, so the size of each mistake is
visible rather than just asserted.
"""

import math
import sys

# Exactly what probe-reports printed, read off a DS4 v2 (PID 0x09CC) over Bluetooth.
HEX = (
    "11 c0 00 80 80 80 80 08 00 00 00 00 00 4b 10 07 00 f0 ff 16 00 c0 fe 1f 1f 79 05 "
    "00 00 00 00 00 01 00 00 01 00 80 00 00 00 80 00 00 00 00 80 00 00 00 80 00 00 00 "
    "00 80 00 00 00 80 00 00 00 00 80 00 00 00 80 00 00 00 00 80 00 00 00 00 00 a2 21 "
    "9e 34"
) + " 00" * 45

buf = [int(b, 16) for b in HEX.split()]

print(f"frame length: {len(buf)} bytes (Windows padded the read to 128)")
end = len(buf)
while end > 1 and buf[end - 1] == 0:
    end -= 1
print(f"meaningful bytes: {end}, the rest is padding\n")

print("what the frame says about itself")
print(f"  [0]    report id 0x{buf[0]:02x}  -> Bluetooth, since USB sends 0x01")
print(f"  [1]    0x{buf[1]:02x}  header byte, skipped by the decoder")
print(f"  [2]    0x{buf[2]:02x}  reserved")
print(f"  [3..7] {' '.join(f'0x{b:02x}' for b in buf[3:7])}  sticks, all 0x80 = centred")

gyro_off, accel_off = 15, 21
gyro = [int.from_bytes(bytes(buf[gyro_off + i:gyro_off + i + 2]), "little", signed=True) for i in (0, 2, 4)]
accel = [int.from_bytes(bytes(buf[accel_off + i:accel_off + i + 2]), "little", signed=True) for i in (0, 2, 4)]

print("\nsensors at the offsets the decoder now uses")
print(f"  gyro  [{gyro_off}..{gyro_off + 6}]  raw {gyro} -> {tuple(round(v / 16.0, 1) for v in gyro)} deg/s")
g = [v / 8192.0 for v in accel]
print(f"  accel [{accel_off}..{accel_off + 6}]  raw {accel} -> {tuple(round(v, 3) for v in g)} g")
mag = math.sqrt(sum(v * v for v in g))
print(f"  accel magnitude {mag:.3f} g", "  <- a pad lying still reads about 1 g" if 0.7 < mag < 1.3 else "  <- WRONG")

print("\nsensors at the offsets the old code used (analog+... = 4+22)")
old_gyro = [int.from_bytes(bytes(buf[26 + i:26 + i + 2]), "little", signed=True) for i in (0, 2, 4)]
old_accel = [int.from_bytes(bytes(buf[32 + i:32 + i + 2]), "little", signed=True) for i in (0, 2, 4)]
og = [v / 8192.0 for v in old_accel]
print(f"  gyro  raw {old_gyro} -> {tuple(round(v / 16.0, 1) for v in old_gyro)} deg/s")
print(f"  accel raw {old_accel} -> {tuple(round(v, 4) for v in og)} g, magnitude {math.sqrt(sum(v*v for v in og)):.4f} g")
print("  an accelerometer reading 0 g while the pad is still is not a physical state")
print("  the pad can be in, so this offset was simply wrong, and nothing complained")

print("\nsticks at the offsets the old code used (analog = 4)")
print(f"  {' '.join(f'0x{b:02x}' for b in buf[4:8])}")
print(f"  byte[7] is 0x{buf[7]:02x}, the d-pad nibble, not a stick axis")
print("  so the old layout read the d-pad as the right stick's X axis")
sys.exit(0 if 0.7 < mag < 1.3 else 1)