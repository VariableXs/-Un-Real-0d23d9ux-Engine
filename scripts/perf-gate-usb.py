#!/usr/bin/env python3
"""任务71（AI-S）· U 盘场景性能基准回归门禁。

总案验收口径：引导 < 8s / 首帧 < 3s / 交互 P95 < 100ms。
数据来源：
  1) 走查脚本落盘的 _attic/p27-28-perf.json（boot_ms + 逐里程碑耗时）；
  2) 基线 _attic/bench/perf-baseline-usb.json（首轮门禁 PASS 时自动写入，
     后续轮次做非劣回归：本轮各指标 ≤ 基线 ×1.2 或显式 FAIL）。

用法：
  python scripts/perf-gate-usb.py                # 断言阈值 + 回归对比
  python scripts/perf-gate-usb.py --update-baseline
退出码：0=门禁 PASS；1=超阈/回归非劣失败；2=输入缺失（先跑 p27-28 走查）。
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PERF_JSON = os.path.join(ROOT, "_attic", "p27-28-perf.json")
BASELINE_JSON = os.path.join(ROOT, "_attic", "bench", "perf-baseline-usb.json")

# 总案阈值（毫秒）。
LIMIT_BOOT_MS = 8000
LIMIT_FIRST_FRAME_MS = 3000
LIMIT_P95_MS = 100
REGRESSION_FACTOR = 1.2  # 回归非劣带


def p95(values):
    if not values:
        return None
    xs = sorted(values)
    idx = min(len(xs) - 1, int(round(0.95 * (len(xs) - 1))))
    return xs[idx]


def main():
    update = "--update-baseline" in sys.argv
    if not os.path.isfile(PERF_JSON):
        print("输入缺失：", PERF_JSON, "——先跑 python _attic/p27-28-shell-demo.py")
        return 2
    with open(PERF_JSON, "r", encoding="utf-8") as f:
        perf = json.load(f)

    boot_ms = perf.get("boot_ms")
    first_frame_ms = perf.get("first_frame_ms")
    steps = perf.get("steps", [])
    by_name = {s["marker"]: s["ms"] for s in steps}
    if first_frame_ms is None:
        # 兼容旧口径（不推荐）：走查步骤耗时（含 TCG 演示链前置）。
        first_frame_ms = by_name.get("SHELL: boot-replay done")
    # 交互口径：除首帧外全部里程碑的 P95（sendkey→打点实测延迟）。
    interact = [s["ms"] for s in steps if s["marker"] != "SHELL: boot-replay done"]
    p95_ms = p95(interact)

    print("=== 任务71 · U 盘场景性能门禁 ===")
    rows = []
    rows.append(("boot_ms", boot_ms, LIMIT_BOOT_MS, boot_ms is not None and boot_ms < LIMIT_BOOT_MS))
    rows.append(("first_frame_ms", first_frame_ms, LIMIT_FIRST_FRAME_MS,
                 first_frame_ms is not None and first_frame_ms < LIMIT_FIRST_FRAME_MS))
    rows.append(("interaction_p95_ms", p95_ms, LIMIT_P95_MS, p95_ms is not None and p95_ms < LIMIT_P95_MS))
    all_ok = True
    for name, v, limit, ok in rows:
        state = "PASS" if ok else ("MISSING" if v is None else "FAIL")
        print(f"  {name:20s} = {v!r:8} limit<{limit:5}  {state}")
        all_ok &= ok
        if v is None:
            all_ok = False

    # 回归对比（基线存在才做；首轮 --update-baseline 落基线）。
    if os.path.isfile(BASELINE_JSON):
        with open(BASELINE_JSON, "r", encoding="utf-8") as f:
            base = json.load(f)
        print("  --- 回归（基线 ×{:.1f} 非劣带） ---".format(REGRESSION_FACTOR))
        for name, v, _, _ in rows:
            b = base.get(name)
            if b is None or v is None:
                continue
            bad = v > b * REGRESSION_FACTOR
            print(f"  {name:20s} base={b:6} now={v:6}  {'REGRESSED' if bad else 'ok'}")
            all_ok &= not bad
    elif update:
        os.makedirs(os.path.dirname(BASELINE_JSON), exist_ok=True)
        base = {name: v for name, v, _, _ in rows if v is not None}
        with open(BASELINE_JSON, "w", encoding="utf-8") as f:
            json.dump(base, f, ensure_ascii=False, indent=1)
        print("  基线已写入:", BASELINE_JSON)
    else:
        print("  （无基线；用 --update-baseline 以本轮 PASS 数据建档）")

    print("PERF GATE:", "PASS" if all_ok else "FAIL")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
