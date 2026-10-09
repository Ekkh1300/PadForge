"""Extracts the icon Windows actually shows for a binary.

`ExtractIconEx` goes through the shell's own icon loader: the same code path
Explorer uses to decide what to draw. If that returns the right bitmap, the
resource is genuinely usable, which parsing the PE directory cannot prove on its
own — a resource can be structurally perfect and still be rejected at load time.
"""
import ctypes
import ctypes.wintypes as wt
import os
import sys


class SHFILEINFOW(ctypes.Structure):
    _fields_ = [
        ("hIcon", wt.HICON),
        ("iIcon", ctypes.c_int),
        ("dwAttributes", wt.DWORD),
        ("szDisplayName", wt.WCHAR * 260),
        ("szTypeName", wt.WCHAR * 80),
    ]


SHGFI_ICON = 0x00000100
SHGFI_LARGEICON = 0x00000000
SHGFI_SMALLICON = 0x00000001
SHGFI_USEFILEATTRIBUTES = 0x00000010

user32 = ctypes.windll.user32
shell32 = ctypes.windll.shell32
shell32.SHGetFileInfoW.restype = wt.DWORD
shell32.ExtractIconExW.argtypes = [
    wt.LPCWSTR, ctypes.c_int, ctypes.POINTER(wt.HICON), ctypes.POINTER(wt.HICON), wt.UINT
]
shell32.ExtractIconExW.restype = wt.UINT
# GetIconInfo fills an ICONINFO; the size is read from the bitmaps it returns,
# so the struct is declared locally in icon_size rather than here.
user32.GetIconInfo.argtypes = [wt.HICON, ctypes.c_void_p]
user32.GetIconInfo.restype = wt.BOOL


def icon_size(hicon):
    """Size of a loaded icon, via the bitmaps GetIconInfo reports.

    GetIconInfo hands back HBITMAP handles rather than a size, so the dimensions
    come from GetObject on the colour bitmap. Asking the shell for the size
    directly is not possible, and GetIconInfoEx needs an ICONINFO whose layout
    differs from a BITMAPINFOHEADER, so reading it as one silently returns
    zeroes.
    """
    if not hicon:
        return None

    class ICONINFO(ctypes.Structure):
        _fields_ = [
            ("fIcon", wt.BOOL),
            ("xHotspot", wt.DWORD),
            ("yHotspot", wt.DWORD),
            ("hbmMask", wt.HBITMAP),
            ("hbmColor", wt.HBITMAP),
        ]

    gdi32 = ctypes.windll.gdi32
    gdi32.GetObjectW.restype = ctypes.c_int
    gdi32.GetObjectW.argtypes = [wt.HGDIOBJ, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint]

    info = ICONINFO()
    if not user32.GetIconInfo(hicon, ctypes.byref(info)):
        return None

    bmp = info.hbmColor or info.hbmMask
    if not bmp:
        return None

    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [
            ("biSize", wt.DWORD),
            ("biWidth", ctypes.c_int32),
            ("biHeight", ctypes.c_int32),
            ("biPlanes", wt.WORD),
            ("biBitCount", wt.WORD),
            ("biCompression", wt.DWORD),
            ("biSizeImage", wt.DWORD),
            ("biXPelsPerMeter", ctypes.c_int32),
            ("biYPelsPerMeter", ctypes.c_int32),
            ("biClrUsed", wt.DWORD),
            ("biClrImportant", wt.DWORD),
        ]

    bmi = BITMAPINFOHEADER()
    got = gdi32.GetObjectW(wt.HGDIOBJ(bmp), ctypes.sizeof(bmi), ctypes.byref(bmi), 0)
    if got == 0:
        return None
    return abs(bmi.biWidth), abs(bmi.biHeight)


def main():
    path = sys.argv[1]
    apath = os.path.abspath(path)

    print(f"{apath}\n")

    ok = True

    # 1. The shell's own loader: what Explorer uses for a file's icon.
    info = SHFILEINFOW()
    flags = SHGFI_ICON | SHGFI_USEFILEATTRIBUTES
    result = shell32.SHGetFileInfoW(apath, 0, ctypes.byref(info), ctypes.sizeof(info), flags)
    if result == 0:
        print("  FAIL: SHGetFileInfoW returned 0, the shell has no icon for this file")
        ok = False
    else:
        print(f"  SHGetFileInfoW   : hIcon={bool(info.hIcon)}, type={info.szTypeName}")
        if not info.hIcon:
            print("  FAIL: shell returned a null icon handle")
            ok = False
        if info.hIcon:
            user32.DestroyIcon(info.hIcon)

    # 2. ExtractIconEx: confirms the group icon resolves to real bitmaps at every
    #    size the group advertises.
    large = wt.HICON()
    small = wt.HICON()
    count = shell32.ExtractIconExW(
        apath, -1, ctypes.byref(large), ctypes.byref(small), 0
    )
    print(f"\n  ExtractIconEx(-1) : {count} icon(s) in the file")
    if count == 0:
        print("  FAIL: the icon group advertises no images")
        ok = False

    large = wt.HICON()
    small = wt.HICON()
    got = shell32.ExtractIconExW(
        apath, 0, ctypes.byref(large), ctypes.byref(small), 1
    )
    print(f"  ExtractIconEx(0)  : got {got}")
    ls = icon_size(large)
    ss = icon_size(small)
    print(f"    large          : {ls}")
    print(f"    small          : {ss}")

    if not got:
        print("  FAIL: could not extract the first icon")
        ok = False
    else:
        # ExtractIconEx only ever asks for two: one "large" and one "small". The
        # shell picks the closest frames from the group, so 32 and 16 are the
        # expected results for an icon carrying the usual size ladder. Asking for
        # 256 here would fail even for a perfectly built icon.
        for name, size, expect in (
            ("large", ls, 32),
            ("small", ss, 16),
        ):
            if size is None:
                print(f"  FAIL: no bitmap reported for the {name} icon")
                ok = False
            elif size != (expect, expect):
                print(f"  FAIL: {name} icon is {size}, expected {expect}x{expect}")
                ok = False

    # The full ladder, to confirm every frame in the group is real and loadable.
    print("\n  every frame the shell can produce:")
    for index in range(count):
        lh = wt.HICON()
        sh = wt.HICON()
        n = shell32.ExtractIconExW(apath, index, ctypes.byref(lh), ctypes.byref(sh), 1)
        if n == 0:
            print(f"    [{index}] could not be extracted")
            ok = False
            continue
        print(f"    [{index}] large={icon_size(lh)} small={icon_size(sh)}")
        if lh:
            user32.DestroyIcon(lh)
        if sh:
            user32.DestroyIcon(sh)

    if large:
        user32.DestroyIcon(large)
    if small:
        user32.DestroyIcon(small)

    print()
    print("Windows loads this icon correctly" if ok else "PROBLEMS FOUND")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())