#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""开始菜单面板：关键矩形精测（面板/搜索框/三栏/卡片/应用行）。"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from png_probe import Png, hexs  # noqa: E402

PANEL = (0x24, 0x24, 0x24)
CARD = (0x2f, 0x2f, 0x2f)


def near(c, t, tol=4):
    return all(abs(c[i] - t[i]) <= tol for i in range(3))


def segs_row(img, y, x0, x1, tol=4):
    out = []
    prev = None
    s = x0
    for x in range(x0, x1):
        c = img.px(x, y)
        if prev is None:
            prev, s = c, x
            continue
        if sum(abs(c[i] - prev[i]) for i in range(3)) >= 6:
            out.append((s, x - 1, prev))
            prev, s = c, x
    out.append((s, x1 - 1, prev))
    return out


def segs_col(img, x, y0, y1):
    out = []
    prev = None
    s = y0
    for y in range(y0, y1):
        c = img.px(x, y)
        if prev is None:
            prev, s = c, y
            continue
        if sum(abs(c[i] - prev[i]) for i in range(3)) >= 6:
            out.append((s, y - 1, prev))
            prev, s = c, y
    out.append((s, y1 - 1, prev))
    return out


def main():
    img = Png(sys.argv[1])

    print("== 1) 搜索框水平范围（y=80 找深色 #202020 内槽）==")
    row = segs_row(img, 80, 0, img.w)
    for a, b, c in row:
        if b - a >= 2:
            print("   %4d..%4d w=%3d %s %s" % (a, b, b - a + 1, hexs(c), "SEARCH" if near(c, (0x20, 0x20, 0x20), 6) else ""))

    print("== 2) 面板垂直边界（列 x=500，找 #242424 起止）==")
    col = segs_col(img, 500, 0, img.h)
    run = []
    for a, b, c in col:
        tag = "PANEL" if near(c, PANEL, 3) else ""
        if b - a >= 1 or tag:
            print("   %4d..%4d h=%3d %s %s" % (a, b, b - a + 1, hexs(c), tag))

    print("== 3) 左栏图标行（列 x=96 找非面板色行程）==")
    col = segs_col(img, 96, 120, img.h)
    for a, b, c in col:
        if not near(c, PANEL, 5) and b - a >= 6:
            print("   y %4d..%4d h=%3d %s" % (a, b, b - a + 1, hexs(c)))

    print("== 4) 中栏卡片垂直边界（列 x=570）==")
    col = segs_col(img, 570, 120, img.h)
    for a, b, c in col:
        if b - a >= 4:
            print("   %4d..%4d h=%3d %s %s" % (a, b, b - a + 1, hexs(c), "CARD" if near(c, CARD, 5) else ""))

    print("== 5) 右栏卡片垂直边界（列 x=840）==")
    col = segs_col(img, 840, 120, img.h)
    for a, b, c in col:
        if b - a >= 4:
            print("   %4d..%4d h=%3d %s %s" % (a, b, b - a + 1, hexs(c), "CARD" if near(c, CARD, 5) else ""))

    print("== 6) 中栏/右栏卡片水平边界（y=460 找 #2f2f2f）==")
    row = segs_row(img, 460, 420, img.w)
    for a, b, c in row:
        if b - a >= 4:
            print("   %4d..%4d w=%3d %s %s" % (a, b, b - a + 1, hexs(c), "CARD" if near(c, CARD, 5) else ""))


main()
