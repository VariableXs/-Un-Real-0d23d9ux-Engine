"""AI-88 机检脚本（PF-299）：性能对标体系 300 项功能清单五断言。

用法（项目根目录）：
    python _attic/tools/ai88_verify.py

五断言：
  1) PF-001..PF-300 唯一且连续零跳号；
  2) 12 个模块齐全且每个 25 项；
  3) 五指标各至少 1 个指标页条目（syscall/启动/内存/IO/帧率）；
  4) GOV-88-J1..J4 判据映射表在册；
  5) 红线条款在册（N/A 禁编造 / 内置盘只读 / NVRAM 只读）。
"""
import re
import sys
from pathlib import Path

DOC = Path(__file__).resolve().parents[2] / (
    "docs/Varix/CoRun Varix STAR II · Unxreal/"
    "CoRun Varix STAR II · Unxreal · AI88 · 内核性能对标体系300项功能清单.md"
)

def main() -> int:
    text = DOC.read_text(encoding="utf-8")
    failures = []

    ids = sorted(set(int(m) for m in re.findall(r"\bPF-(\d{3})\b", text)))
    if ids != list(range(1, 301)):
        missing = [i for i in range(1, 301) if i not in ids]
        failures.append(f"A1 失败：unique={len(ids)} missing={missing}")

    mods = re.findall(r"### 模块 ([A-L]) ·", text)
    per_mod = [0] * 12
    for i in range(1, 301):
        if re.search(rf"\| PF-{i:03d} \| ", text):
            per_mod[(i - 1) // 25] += 1
    if len(mods) != 12 or any(c != 25 for c in per_mod):
        failures.append(f"A2 失败：模块数={len(mods)} 各模块计数={per_mod}")

    pages = ["账本第一指标页", "启动账模板", "内存五指标页", "IO 五指标页", "帧率账…|账本第五指标页|帧账发布页"]
    if not all(re.search(p, text) for p in pages):
        failures.append("A3 失败：五指标页条目缺项")

    if not all(f"GOV-88-J{i}" in text for i in range(1, 5)):
        failures.append("A4 失败：GOV-88 判据映射缺项")

    for kw in ("不许编造", "只读", "NVRAM"):
        if kw not in text:
            failures.append(f"A5 失败：红线关键词缺「{kw}」")

    if failures:
        print("\n".join(failures))
        return 1
    print("AI88 五断言 ALL PASS：300 项连续零跳号 / 12 模块×25 项 / 五指标页在册 / GOV-88-J1~J4 在册 / 红线条款在册")
    return 0

if __name__ == "__main__":
    sys.exit(main())
