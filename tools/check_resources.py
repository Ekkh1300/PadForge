"""Confirms a PE file actually carries an icon and version resource.

The build prints no warning when windres succeeds, and "it compiled" is not the
same as "Explorer will show it". This reads the PE directory directly, so it
checks the artefact rather than the intention.

The resource tree is three levels: type -> id -> language -> data entry. The
walk is explicit about that, because collapsing the levels loses the type id and
RT_ICON becomes indistinguishable from RT_GROUP_ICON.

Run:  python check_resources.py path/to/binary.exe
"""
import struct
import sys

RT_ICON = 3
RT_GROUP_ICON = 14
RT_VERSION = 16
RT_MANIFEST = 24


class Pe:
    def __init__(self, path):
        with open(path, "rb") as fh:
            self.data = fh.read()
        self.path = path
        self._parse()

    def _parse(self):
        d = self.data
        if d[:2] != b"MZ":
            raise SystemExit(f"{self.path} is not a PE file")
        pe_off = struct.unpack_from("<I", d, 0x3C)[0]
        if d[pe_off : pe_off + 4] != b"PE\x00\x00":
            raise SystemExit(f"{self.path} has no PE signature")

        coff = pe_off + 4
        self.sections = struct.unpack_from("<H", d, coff + 2)[0]
        opt_size = struct.unpack_from("<H", d, coff + 16)[0]
        opt = coff + 20
        if struct.unpack_from("<H", d, opt)[0] != 0x20B:
            raise SystemExit(f"{self.path} is not 64-bit PE")

        # Data directories begin at opt+112 for PE32+; the count sits just before.
        dirs = opt + 112
        num_dirs = struct.unpack_from("<I", d, dirs - 4)[0]
        if num_dirs > 2:
            self.resource_rva, self.resource_size = struct.unpack_from("<II", d, dirs + 2 * 8)
        else:
            self.resource_rva, self.resource_size = 0, 0

        # Section table follows the optional header.
        self.sec_table = opt + opt_size

    def rva_to_offset(self, rva):
        """Map a virtual address to a file offset.

        Section header after the 8-byte name is virtual size, virtual address,
        raw size, raw pointer, in that order. Getting this order wrong is easy
        and yields an RVA that appears to map to itself, which then reads
        plausible-looking garbage instead of failing loudly.
        """
        for i in range(self.sections):
            base = self.sec_table + i * 40
            vsize, virt, rsize, raw = struct.unpack_from("<IIII", self.data, base + 8)
            if virt <= rva < virt + max(vsize, rsize):
                return raw + (rva - virt)
        return None

    def dir_entries(self, base, offset):
        """Yield (id, child_offset, is_directory) for one IMAGE_RESOURCE_DIRECTORY."""
        d = self.data
        _chars, _stamp, _major, _minor, named, ids = struct.unpack_from("<IIHHHH", d, base + offset)
        pos = base + offset + 16
        for _ in range(named + ids):
            name_or_id, child = struct.unpack_from("<II", d, pos)
            pos += 8
            is_dir = bool(child & 0x80000000)
            yield name_or_id, child & 0x7FFFFFFF, is_dir

    def data_entry(self, base, offset):
        rva, size, _codepage, _reserved = struct.unpack_from("<IIII", self.data, base + offset)
        return rva, size

    def collect(self):
        """Return {type_id: [(data_rva, size), ...]} by walking the tree."""
        if self.resource_rva == 0:
            return {}
        base = self.rva_to_offset(self.resource_rva)
        if base is None:
            return {}

        out = {}
        for type_id, type_off, type_is_dir in self.dir_entries(base, 0):
            if not type_is_dir:
                continue
            entries = []
            for _id, id_off, id_is_dir in self.dir_entries(base, type_off):
                if not id_is_dir:
                    entries.append(self.data_entry(base, id_off))
                    continue
                for _lang, lang_off, lang_is_dir in self.dir_entries(base, id_off):
                    if not lang_is_dir:
                        entries.append(self.data_entry(base, lang_off))
            out[type_id] = entries
        return out


def read_version_strings(pe, rva, size):
    off = pe.rva_to_offset(rva)
    if off is None:
        return {}
    blob = pe.data[off : off + size]
    found = {}
    for field in (
        "ProductName",
        "FileVersion",
        "CompanyName",
        "FileDescription",
        "LegalCopyright",
    ):
        needle = field.encode("utf-16-le")
        i = blob.find(needle)
        if i < 0:
            continue
        # After the key comes its wLength, then padding to a 4-byte boundary.
        j = (i + len(needle) + 2 + 3) & ~3
        end = blob.find(b"\x00\x00", j)
        if end < 0:
            continue
        found[field] = blob[j:end].decode("utf-16-le", errors="replace").rstrip("\x00")
    return found


def main():
    path = sys.argv[1]
    pe = Pe(path)

    print(f"{path}")
    print(f"  PE sections      : {pe.sections}")
    print(f"  resource section : RVA {pe.resource_rva:#x}, {pe.resource_size} bytes")

    if pe.resource_rva == 0:
        print("\n  FAIL: no resource section, so no icon and no version info")
        return 1

    tree = pe.collect()
    icons = tree.get(RT_ICON, [])
    groups = tree.get(RT_GROUP_ICON, [])
    versions = tree.get(RT_VERSION, [])
    manifests = tree.get(RT_MANIFEST, [])

    print(f"  RT_ICON frames   : {len(icons)}")
    print(f"  RT_GROUP_ICON    : {len(groups)}")
    print(f"  RT_VERSION       : {len(versions)}")
    print(f"  RT_MANIFEST      : {len(manifests)}")

    ok = True

    if not groups:
        print("  FAIL: no RT_GROUP_ICON, so Explorer shows a generic glyph")
        ok = False

    if groups:
        rva, size = groups[0]
        off = pe.rva_to_offset(rva)
        if off is None:
            print("  FAIL: group icon is not inside any section")
            ok = False
        else:
            _reserved, kind, count = struct.unpack_from("<HHH", pe.data, off)
            print(f"  group icon type  : {kind}")
            print(f"  group icon images: {count}")
            if kind != 1:
                print("  FAIL: group icon type must be 1")
                ok = False
            if count != len(icons):
                print(f"  FAIL: group lists {count} but {len(icons)} frames exist")
                ok = False
            else:
                # GRPICONDIRENTRY is 14 bytes: width, height, colour count and
                # reserved as bytes, planes as a word, then dwBytesInRes and nID
                # as dwords. The bit count is not stored per entry; it lives in
                # each RT_ICON frame's own DIB header. There is also no offset
                # field, unlike the 16-byte ICONDIRENTRY in the .ico file.
                #
                # Reading this with the wrong stride makes every frame after the
                # first look like garbage, which is a confusing way to discover
                # the layout is wrong.
                entry = struct.calcsize("<BBBBHII")
                assert entry == 14, f"GRPICONDIRENTRY must be 14 bytes, got {entry}"
                for i in range(count):
                    w, h, _colours, _reserved, _planes, bsize, _nid = struct.unpack_from(
                        "<BBBBHII", pe.data, off + 6 + i * 14
                    )
                    print(f"    frame {i}: {w or 256:>3}x{h or 256:<3} {bsize:>6} bytes")

    if versions:
        rva, size = versions[0]
        strings = read_version_strings(pe, rva, size)
        if not strings:
            print("  WARN: RT_VERSION present but no readable fields")
        for key, value in strings.items():
            print(f"  {key:<18} {value}")
        for required in ("ProductName", "FileVersion"):
            if required not in strings:
                print(f"  FAIL: {required} missing from the version resource")
                ok = False
    else:
        print("  FAIL: no RT_VERSION, so Properties shows no version")
        ok = False

    print()
    print("resources present and consistent" if ok else "PROBLEMS FOUND")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())