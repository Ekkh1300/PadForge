"""Print the captured frame as a Rust byte array, ready to paste.

Kept separate from the layout checker so the fixture can be regenerated without
re-deriving the reasoning. The bytes are the ones probe-reports printed from a
DS4 v2 over Bluetooth.
"""

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

rows = []
for i in range(0, end, 12):
    rows.append("        " + " ".join(f"0x{v:02x}," for v in buf[i:min(i + 12, end)]))

print(f"pub(crate) const CAPTURED_BT_REPORT: [u8; {end}] = [")
print("\n".join(rows))
print("    ];")