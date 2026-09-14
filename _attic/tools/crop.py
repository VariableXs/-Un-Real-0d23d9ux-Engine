#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""PNG 裁剪工具（纯 Python，无依赖）：用于把截图按原始分辨率切块观察。

非功能性过程脚本，归档在 _attic/ 下。
"""
import sys
import zlib
import struct
from png_probe import Png


def write_png(path, w, h, rgb_rows):
    raw = bytearray()
    for row in rgb_rows:
        raw.append(0)
        raw += row
    comp = zlib.compress(bytes(raw), 9)

    def chunk(typ, data):
        return (struct.pack(">I", len(data)) + typ + data
                + struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF))

    out = b"\x89PNG\r\n\x1a\n"
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    out += chunk(b"IDAT", comp)
    out += chunk(b"IEND", b"")
    open(path, "wb").write(out)


def main():
    src, dst, x0, y0, x1, y1 = sys.argv[1], sys.argv[2], *(int(v) for v in sys.argv[3:7])
    img = Png(src)
    x1 = min(x1, img.w); y1 = min(y1, img.h)
    rows = []
    for y in range(y0, y1):
        row = bytearray()
        for x in range(x0, x1):
            r, g, b = img.px(x, y)
            row += bytes((r, g, b))
        rows.append(row)
    write_png(dst, x1 - x0, y1 - y0, rows)
    print("WROTE %s %dx%d" % (dst, x1 - x0, y1 - y0))


if __name__ == "__main__":
    main()
