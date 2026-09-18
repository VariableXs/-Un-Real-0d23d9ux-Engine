#!/usr/bin/env python3
"""验收轮 · 8 屏像素带复验（PIL 逐行亮像素带分析，唯一可信判据）。

逐行求灰度 max，>阈值判亮行，连续亮行合并为带；输出各屏带列表供与
ushell 绘制坐标对账（icons 文字 y=44/116/188、菜单文字 y=608/656/704、
任务栏 y=768-783、文件行 y=96+28n、设置项 y=126+56n 等）。
"""
import os
import sys
from PIL import Image

SHOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                    "_attic", "acceptance-p27")
TH = int(sys.argv[1]) if len(sys.argv) > 1 else 170

# 各屏的期望亮带（y 区间，来自 ushell 绘制坐标换算；容忍 ±4px）。
EXPECT = {
    "02-desktop.png":   [(40, 62), (112, 133), (184, 205), (764, 787)],
    "03-startmenu.png": [(40, 62), (112, 133), (184, 205), (604, 716), (764, 787)],
    "04-files.png":     [(60, 80), (92, 250), (654, 676), (764, 787)],
    "05-fileview.png":  [(60, 80), (92, 400), (654, 676), (764, 787)],
    "06-settings.png":  [(80, 100), (122, 132), (178, 188), (234, 244), (348, 370), (764, 787)],
    "07-settings-changed.png": [(80, 100), (122, 132), (178, 188), (234, 244), (348, 370), (764, 787)],
    "08-about.png":     [(80, 100), (112, 240), (330, 352), (764, 787)],
}


def bands(path, th):
    im = Image.open(path).convert("L")
    w, h = im.size
    rows = []
    for y in range(h):
        line = im.crop((0, y, w, y + 1))
        if line.getextrema()[1] > th:
            rows.append(y)
    out = []
    for y in rows:
        if out and y - out[-1][1] <= 2:
            out[-1][1] = y
        else:
            out.append([y, y])
    return [(a, b) for a, b in out if b - a >= 2]


def main():
    total = miss = 0
    for name, exp in EXPECT.items():
        p = os.path.join(SHOT, name)
        if not os.path.exists(p):
            print(f"{name}: MISSING FILE")
            miss += len(exp)
            total += len(exp)
            continue
        got = bands(p, TH)
        got_s = ", ".join(f"({a},{b})" for a, b in got)
        print(f"{name}: {len(got)} bands: {got_s}")
        for (ea, eb) in exp:
            total += 1
            hit = any(a <= eb + 4 and b >= ea - 4 for (a, b) in got)
            if not hit:
                miss += 1
                print(f"    MISSING expected band ({ea},{eb})")
    print(f"\nBAND CHECK: {total - miss}/{total} expected bands present, TH={TH}")
    return 0 if miss == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
