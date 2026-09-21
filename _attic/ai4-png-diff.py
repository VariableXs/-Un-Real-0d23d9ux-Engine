# -*- coding: utf-8 -*-
"""AI-4 · S2.06 逐屏对照设施：Windows 侧 vs 内核侧截图差异报告。

用法：python _attic/ai4-png-diff.py <win.png> <kernel.png> [--grid 16] [--out report.md]
- 尺寸不同时按内核侧分辨率归一（DPI/分辨率差异先归一再比）；
- 输出：整体相似度 %、逐网格差异热区（差异像素占比>15% 的格子标红）、
  最大差异区块坐标——Servo 接入后逐屏对照（C1 卡）的直接度量器。
依赖：Pillow（PIL）。
"""
import sys

try:
    from PIL import Image, ImageChops
except ImportError:
    print("PIL 缺失：pip install pillow")
    sys.exit(2)


def load(path, size=None):
    im = Image.open(path).convert("RGB")
    if size and im.size != size:
        im = im.resize(size)
    return im


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) < 2:
        print(__doc__)
        sys.exit(2)
    grid_n = 16
    if "--grid" in sys.argv:
        grid_n = int(sys.argv[sys.argv.index("--grid") + 1])
    out_path = None
    if "--out" in sys.argv:
        out_path = sys.argv[sys.argv.index("--out") + 1]

    k = load(args[1])
    w = load(args[0], k.size)  # 归一到内核侧尺寸
    if w.size != k.size:
        w = w.resize(k.size)

    diff = ImageChops.difference(w, k).convert("L")
    px = list(diff.getdata())
    total = len(px)
    # 感知阈值：≤24 的差视为渲染噪声（抗锯齿/字体微差）。
    sig = sum(1 for v in px if v > 24)
    sim = 100.0 * (1 - sig / total)

    lines = [
        "# S2.06 逐屏对照报告（AI-4 png-diff）",
        f"- Windows 侧: {args[0]}",
        f"- 内核侧:    {args[1]}",
        f"- 尺寸: {k.size[0]}x{k.size[1]}（归一后对比）",
        f"- **相似度: {sim:.2f}%**（显著差异像素 {sig}/{total}，阈值 24）",
        f"- 判定: {'PASS（≥97%）' if sim >= 97 else 'CONCERN（90-97%）' if sim >= 90 else 'FAIL（<90%）'}",
        "",
        "## 差异热区（网格 {}x{}，占比>15% 标红）".format(grid_n, grid_n),
        "",
    ]
    gw, gh = k.size[0] // grid_n, k.size[1] // grid_n
    hotspots = []
    for gy in range(grid_n):
        row = []
        for gx in range(grid_n):
            box = (gx * gw, gy * gh, (gx + 1) * gw, (gy + 1) * gh)
            cell = list(diff.crop(box).getdata())
            d = sum(1 for v in cell if v > 24) / len(cell)
            row.append("▓" if d > 0.15 else ("▒" if d > 0.05 else "·"))
            if d > 0.15:
                hotspots.append((gx, gy, d))
        lines.append("".join(row))
    if hotspots:
        lines.append("")
        lines.append("## 最大差异区块 TOP10")
        for gx, gy, d in sorted(hotspots, key=lambda x: -x[2])[:10]:
            lines.append(f"- 格 ({gx},{gy}) 差异占比 {d:.0%} —— 屏幕区域 ({gx * gw},{gy * gh})~({(gx + 1) * gw},{(gy + 1) * gh})")

    report = "\n".join(lines)
    print(report)
    if out_path:
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(report + "\n")
        print(f"\n报告已写: {out_path}")


if __name__ == "__main__":
    main()
