#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""包围盒 / 矩形探测：量出真实 Win11 截图里各元素的精确像素盒。

非功能性过程脚本，归档在 _attic/ 下。
"""
import sys
from png_probe import Png, hexs


def bbox(img, x0, y0, x1, y1, pred):
    xs, ys = [], []
    for y in range(y0, min(y1, img.h)):
        for x in range(x0, min(x1, img.w)):
            if pred(img.px(x, y)):
                xs.append(x); ys.append(y)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs), max(ys))


def near(c, t, tol=18):
    return all(abs(c[i] - t[i]) <= tol for i in range(3))


def bright(c, thr=90):
    return (c[0] + c[1] + c[2]) / 3 >= thr


def main():
    img = Png(sys.argv[1])
    lab = sys.argv[2] if len(sys.argv) > 2 else "all"
    P = lambda *a: print(*a)

    if lab in ("all", "text"):
        # 导航页签文字「主页」：放大包围盒（CJK 字形高 ≈ 0.88em）
        P("nav '主页' bbox:", bbox(img, 70, 165, 120, 215, lambda c: bright(c, 110)))
        P("nav '系统' bbox:", bbox(img, 70, 215, 120, 265, lambda c: bright(c, 110)))
        P("nav '蓝牙和其他设备' bbox:", bbox(img, 70, 265, 260, 62 + 260, lambda c: bright(c, 110)))
        # 页面标题「主页」
        P("page title bbox:", bbox(img, 890, 80, 1030, 150, lambda c: bright(c, 140)))
        # 卡片标题「推荐设置」
        P("card head '推荐设' bbox:", bbox(img, 690, 300, 830, 345, lambda c: bright(c, 140)))
        # 行文字「屏幕」
        P("row text bbox:", bbox(img, 690, 430, 760, 470, lambda c: bright(c, 130)))

    if lab in ("all", "nav"):
        # 选中导航项填充矩形（#2b2d31）
        P("nav sel rect:", bbox(img, 0, 160, 420, 240, lambda c: near(c, (0x2b, 0x2d, 0x31), 4)))
        P("nav bg:", hexs(img.avg(300, 700)), "content bg:", hexs(img.avg(700, 200)))
        # 头像
        P("avatar bbox:", bbox(img, 0, 60, 120, 160, lambda c: bright(c, 120)))

    if lab in ("all", "card"):
        # 卡片 1（推荐设置）
        P("card1 rect:", bbox(img, 600, 260, 1140, 690, lambda c: near(c, (0x26, 0x2b, 0x3b), 5)))
        # 卡片行分隔线：扫描 x=900 的列，找卡片内的横向分隔
        col = [img.px(900, y) for y in range(260, 700)]
        prev = col[0]
        for i, c in enumerate(col):
            if sum(abs(c[k] - prev[k]) for k in range(3)) >= 6:
                P("   card1 edge y=%d %s -> %s" % (260 + i, hexs(prev), hexs(c)))
            prev = c

    if lab in ("all", "title"):
        # 标题栏：搜索框
        P("searchbox rect:", bbox(img, 560, 0, 1350, 80,
                                  lambda c: near(c, (0x29, 0x2d, 0x35), 8)))
        # 窗口按钮：右上角较亮或图形
        row = [img.px(x, 46) for x in range(1560, img.w)]
        prev = row[0]
        for i, c in enumerate(row):
            if sum(abs(c[k] - prev[k]) for k in range(3)) >= 14:
                P("   caption edge x=%d %s -> %s" % (1560 + i, hexs(prev), hexs(c)))
            prev = c
        P("caption glyph band y=30..46 v-scan:")
        for x in (1740, 1780, 1820, 1860, 1900):
            col = [img.px(x, y) for y in range(20, 70)]
            P("   x=%d %s" % (x, " ".join(hexs(c) for c in col[::6])))


if __name__ == "__main__":
    main()
