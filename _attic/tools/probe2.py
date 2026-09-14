#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""定向几何探针：Win11 设置窗口的搜索框 / 导航项 / 卡片边界与配色。

非功能性过程脚本，归档在 _attic/ 下。
"""
import sys
from png_probe import Png, hexs


def runs(vals, thr=8):
    """把一维颜色序列压成 [(start, end, color)] 行程，忽略 <thr 的微差。"""
    out = []
    start = 0
    base = vals[0]
    for i in range(1, len(vals)):
        d = sum(abs(vals[i][k] - base[k]) for k in range(3))
        if d >= thr:
            out.append((start, i - 1, base))
            start = i
            base = vals[i]
    out.append((start, len(vals) - 1, base))
    return out


def show_runs(label, vals, thr=10, minlen=2, x_off=0):
    print("\n-- %s --" % label)
    for (a, b, c) in runs(vals, thr):
        if b - a + 1 >= minlen:
            print("   %s..%s (len %d) %s" % (a + x_off, b + x_off, b - a + 1, hexs(c)))


def main():
    img = Png(sys.argv[1])
    W, H = img.w, img.h
    print("SIZE %dx%d" % (W, H))

    # 顶部区域（标题栏/搜索框）行扫描
    for y in (20, 30, 40, 50, 60, 70, 80, 90, 100, 110):
        row = [img.px(x, y) for x in range(0, W, 1)]
        print("\n=== ROW y=%d ===" % y)
        for (a, b, c) in runs(row, 12):
            if b - a + 1 >= 6:
                print("   x %4d..%4d (w %4d) %s" % (a, b, b - a + 1, hexs(c)))

    # 列扫描：找标题栏底边 / 内容起点
    print("\n=== COL x=200（穿过左导航）===")
    col = [img.px(200, y) for y in range(0, H)]
    for (a, b, c) in runs(col, 12):
        if b - a + 1 >= 3:
            print("   y %4d..%4d (h %4d) %s" % (a, b, b - a + 1, hexs(c)))

    print("\n=== COL x=900（穿过内容区卡片）===")
    col = [img.px(900, y) for y in range(0, H)]
    for (a, b, c) in runs(col, 12):
        if b - a + 1 >= 3:
            print("   y %4d..%4d (h %4d) %s" % (a, b, b - a + 1, hexs(c)))


if __name__ == "__main__":
    main()
