#!/usr/bin/env python3
r"""gen-numbers.py · AURORA-1000 功能计数生成器（步骤 0258）。

扫描 kernel/varix/src/aurora/*.rs 中的功能编号注释（A\d+ / W1-I\d+），
输出各域计数与 W1 总数（应为 200：AI-01~AI-08 各 25）。
用法：python scripts/gen-numbers.py
"""
import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "kernel" / "varix" / "src" / "aurora"

# W1 = AI-01~AI-08（A001~A200）
W1_FILES = [
    "display.rs", "render2d.rs", "typography.rs", "gpu.rs",
    "compositor.rs", "image.rs", "input.rs", "audio.rs",
]
W2_FILES = ["window.rs", "motion.rs", "desktop.rs", "appfw.rs"]


def count_features(path: Path) -> int:
    text = path.read_text(encoding="utf-8")
    codes = set(re.findall(r"\bA(\d{3})\b", text))
    return len(codes)


def main() -> int:
    total_w1 = 0
    rows = []
    for name in W1_FILES + W2_FILES + ["widgets.rs", "clipboard.rs", "session.rs"]:
        p = SRC / name
        n = count_features(p) if p.exists() else 0
        rows.append((name, n))
    for name, n in rows:
        print(f"{name:20s} {n:3d}")
    total_w1 = sum(n for name, n in rows if name in W1_FILES)
    print(f"W1 total (A001~A200): {total_w1}")
    if total_w1 != 200:
        print("FAIL: W1 must be 200", file=sys.stderr)
        return 1
    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
