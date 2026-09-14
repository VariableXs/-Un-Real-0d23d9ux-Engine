#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""开始菜单（点 V 字后）面板几何探针：量出面板/搜索框/三栏/卡片/行高的精确像素。

用法: python probe_start.py <png> [mode]
非功能性过程脚本，按项目纪律归档在 _attic/ 下。
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from png_probe import Png, hexs  # noqa: E402


def rdraw(img, y, x0, x1, thr=10):
    """沿一行找色带（行程编码）。"""
    out = []
    prev = None
    s = x0
    for x in range(x0, x1):
        c = img.px(x, y)
        if prev is not None and sum(abs(c[i] - prev[i]) for i in range(3)) >= thr:
            out.append((s, x - 1, prev))
            s = x
        prev = c
    out.append((s, x1 - 1, prev))
    return [seg for seg in out if seg[1] - seg[0] >= 2]


def cdraw(img, x, y0, y1, thr=10):
    out = []
    prev = None
    s = y0
    for y in range(y0, y1):
        c = img.px(x, y)
        if prev is not None and sum(abs(c[i] - prev[i]) for i in range(3)) >= thr:
            out.append((s, y - 1, prev))
            s = y
        prev = c
    out.append((s, y1 - 1, prev))
    return [seg for seg in out if seg[1] - seg[0] >= 2]


def main():
    img = Png(sys.argv[1])
    mode = sys.argv[2] if len(sys.argv) > 2 else "size"
    print("IMAGE %dx%d colorType=%d" % (img.w, img.h, img.colort))

    if mode == "size":
        return

    if mode == "panel":
        # 面板背景色（从图中心取）
        for y in (400, 500, 600):
            print("row y=%d" % y)
            for a, b, c in rdraw(img, y, 0, img.w, 8):
                print("   %4d..%4d w=%3d %s" % (a, b, b - a + 1, hexs(c)))
        print("col x=%d" % (img.w // 2))
        for a, b, c in cdraw(img, img.w // 2, 0, img.h, 8):
            print("   %4d..%4d h=%3d %s" % (a, b, b - a + 1, hexs(c)))

    if mode == "search":
        # 搜索框上下边界（沿 x=120 竖扫）
        for x in (120, 400, 900):
            print("col x=%d" % x)
            for a, b, c in cdraw(img, x, 30, 120, 6):
                print("   %4d..%4d h=%3d %s" % (a, b, b - a + 1, hexs(c)))

    if mode == "cols":
        for y in (200, 300, 500, 700, 800):
            print("row y=%d" % y)
            for a, b, c in rdraw(img, y, 0, img.w, 8):
                print("   %4d..%4d w=%3d %s" % (a, b, b - a + 1, hexs(c)))

    if mode == "rows":
        # 左栏应用行：沿 x=110 竖扫找图标行
        for x in (110, 150):
            print("col x=%d" % x)
            for a, b, c in cdraw(img, x, 150, img.h, 24):
                print("   %4d..%4d h=%3d %s" % (a, b, b - a + 1, hexs(c)))


main()
