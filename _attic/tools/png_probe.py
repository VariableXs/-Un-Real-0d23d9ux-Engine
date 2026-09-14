#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""纯 Python PNG 解码 + Windows 11 设置窗口几何/取色探针（无第三方依赖）。

用途：从真实 Win11 设置截图里量出精确像素尺寸与颜色，避免"凭印象写数值"。
非功能性过程脚本，按项目纪律归档在 _attic/ 下。
"""
import zlib
import struct
import sys


class Png:
    def __init__(self, path):
        data = open(path, "rb").read()
        assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a png"
        pos, idat = 8, bytearray()
        self.w = self.h = self.bitd = self.colort = 0
        self.interlace = 0
        plte = None
        while pos < len(data):
            (ln,) = struct.unpack(">I", data[pos:pos + 4])
            typ = data[pos + 4:pos + 8]
            chunk = data[pos + 8:pos + 8 + ln]
            if typ == b"IHDR":
                (self.w, self.h, self.bitd, self.colort, _c, _f,
                 self.interlace) = struct.unpack(">IIBBBBB", chunk)
            elif typ == b"IDAT":
                idat += chunk
            elif typ == b"PLTE":
                plte = chunk
            elif typ == b"IEND":
                break
            pos += 12 + ln
        assert self.bitd == 8, "only 8-bit supported, got %d" % self.bitd
        assert self.interlace == 0, "interlaced unsupported"
        ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[self.colort]
        self.ch = ch
        self.plte = plte
        raw = zlib.decompress(bytes(idat))
        stride = self.w * ch
        out = bytearray(stride * self.h)
        prev = bytearray(stride)
        p = 0
        for y in range(self.h):
            f = raw[p]
            p += 1
            line = bytearray(raw[p:p + stride])
            p += stride
            if f == 1:
                for i in range(ch, stride):
                    line[i] = (line[i] + line[i - ch]) & 0xFF
            elif f == 2:
                for i in range(stride):
                    line[i] = (line[i] + prev[i]) & 0xFF
            elif f == 3:
                for i in range(stride):
                    a = line[i - ch] if i >= ch else 0
                    line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
            elif f == 4:
                for i in range(stride):
                    a = line[i - ch] if i >= ch else 0
                    b = prev[i]
                    c = prev[i - ch] if i >= ch else 0
                    pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                    pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                    line[i] = (line[i] + pr) & 0xFF
            out[y * stride:(y + 1) * stride] = line
            prev = line
        self.pix = out
        self.stride = stride

    def px(self, x, y):
        i = y * self.stride + x * self.ch
        p = self.pix
        if self.colort == 3:
            idx = p[i]
            return (self.plte[idx * 3], self.plte[idx * 3 + 1], self.plte[idx * 3 + 2])
        if self.ch == 1:
            v = p[i]
            return (v, v, v)
        if self.ch == 2:
            v = p[i]
            return (v, v, v)
        return (p[i], p[i + 1], p[i + 2])

    def avg(self, x, y, r=2):
        n = s = 0
        acc = [0, 0, 0]
        for yy in range(max(0, y - r), min(self.h, y + r + 1)):
            for xx in range(max(0, x - r), min(self.w, x + r + 1)):
                c = self.px(xx, yy)
                acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]
                n += 1
        return tuple(v // n for v in acc)


def hexs(c):
    return "#%02x%02x%02x" % c


def main():
    path = sys.argv[1]
    img = Png(path)
    print("SIZE %dx%d colort=%d ch=%d" % (img.w, img.h, img.colort, img.ch))

    # ---- 1) 逐行扫描：找出窗口上下边界（相对桌面壁纸的色差）----
    print("\n== 行亮度剖面（每 10 行采样，取 x=4 与 x=w-4 两点）==")
    for y in range(0, img.h, 10):
        l = img.px(4, y)
        r = img.px(img.w - 4, y)
        print("  y=%4d L%s R%s" % (y, hexs(l), hexs(r)))

    # ---- 2) 横向边界：找导航栏与内容区的分界（列剖面）----
    print("\n== 列剖面（y=窗口中部，找分隔线）==")
    ymid = int(img.h * 0.55)
    prev = None
    for x in range(0, img.w):
        c = img.px(x, ymid)
        if prev is not None:
            d = sum(abs(c[i] - prev[i]) for i in range(3))
            if d >= 12:
                print("  x=%4d %s -> %s (d=%d)" % (x, hexs(prev), hexs(c), d))
        prev = c
    print("  ymid=%d color=%s" % (ymid, hexs(img.px(int(img.w * 0.5), ymid))))


if __name__ == "__main__":
    main()
