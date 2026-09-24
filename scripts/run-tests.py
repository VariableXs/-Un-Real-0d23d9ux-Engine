#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""大测试总闸 · run-tests（Variable 明令整合，2026-09-24）

统一入口，收编 _attic 与 scripts/ 的全部历史战役脚本。三档速度：

  fast    宿主侧全量单测（cargo test，分钟级，日常跑）
  selftest已登记的自检类脚本（宿主侧，秒~分钟级）
  full    fast + selftest + QEMU 批队列（闸门跑，可过夜无人值守）
  single  按文件名单跑任一已收编脚本
  list    列出收编台账（分类/运行时/安全级别）
  regen   重扫 _attic/ 与 scripts/，重建注册表（保留手工覆盖）

安全铁律（硬件与数据安全红线，2026-09-23 入格）：
  - danger 类（写盘/写引导/提权/部署类脚本）永远不进 fast/selftest/full；
  - single 跑 danger 类必须显式 --yes，且打印红线警告与改动清单提示；
  - 本总闸自身对磁盘零写入（日志除外，只写 _attic/run-tests-logs/）。

用法：
  python scripts/run-tests.py fast [--dry-run]
  python scripts/run-tests.py selftest
  python scripts/run-tests.py full  [--dry-run]
  python scripts/run-tests.py single <文件名> [--args ...] [--yes]
  python scripts/run-tests.py list [--class CLASS]
  python scripts/run-tests.py regen
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ATTIC = REPO / "_attic"
SCRIPTS = REPO / "scripts"
REGISTRY = SCRIPTS / "test-registry.json"
LOGROOT = ATTIC / "run-tests-logs"

# ---------------------------------------------------------------- 分类规则
# 顺序即优先级：danger 最先（保守归类），随后 selftest / qemu / demo / probe，
# 都不中 = oneshot（一次性战役残件，总闸默认不跑）。
DANGER_PATTERNS = [
    r"-elevated", r"^bcd-", r"deploy", r"vx-fix", r"fix-0xed", r"rm-letter",
    r"kern-update", r"uefi-vhd", r"esp-deploy", r"esp-win-boot-provision",
    r"esp-deploy-switch", r"hiberboot", r"hiberfix", r"gpt-repair",
    r"vx-bcdraw", r"autostart", r"runkeys", r"w1-launch", r"w1-finalize",
    r"w1-write", r"w1-monitor", r"w1-marker", r"postlogin", r"profdir",
    r"vx-gpt", r"esp-dump", r"verify-deploy", r"cleanup-", r"patch_",
    r"^patch-", r"^fix\d", r"^p37-patch", r"^append-", r"^update-",
    r"add-leak-test", r"fix-ushell", r"fix-winsurf", r"fix_quota",
    r"fix_probe", r"fix_backslash", r"bios-load-sweep", r"usb-fw-entry",
    r"shared-cfg", r"splat-exp", r"display-probe-drive", r"input-probe-drive",
]
SELFTEST_PATTERNS = [r"selftest"]
QEMU_PATTERNS = [
    r"qemu", r"^hmp", r"t7-t8", r"walkthrough", r"campaign",
    r"unplug-drill", r"fs23-powercut", r"p58-pull-chain", r"^panic-drill",
    r"chaos-drill", r"^run-qemu", r"panic-forensics",
]
DEMO_PATTERNS = [r"demo"]
PROBE_PATTERNS = [
    r"probe", r"diag", r"^list-", r"readonly", r"ro_health", r"-check",
    r"verify", r"parse", r"enum", r"survey", r"inventory", r"vol-check",
    r"preflight", r"postverify", r"bandcheck", r"plan-check", r"plan-check",
    r"gpt-readonly", r"health", r"status", r"smoke", r"walk", r"s111",
    r"accept-", r"t1-env", r"t6-", r"t8-realboot", r"p60-machine-matrix",
]

CLASS_META = {
    "selftest": ("宿主", "安全", "已登记自检，可日常跑"),
    "probe":    ("宿主", "安全", "只读探针/诊断"),
    "demo":     ("宿主", "安全", "功能演示"),
    "qemu":     ("QEMU", "安全(慢)", "仿真战役，闸门/过夜跑"),
    "oneshot":  ("宿主", "存档", "一次性战役残件，总闸不跑"),
    "danger":   ("宿主", "⚠危险", "写盘/提权/部署类，single 需 --yes"),
}


def classify(name: str) -> str:
    low = name.lower()
    for p in DANGER_PATTERNS:
        if re.search(p, low):
            return "danger"
    for p in SELFTEST_PATTERNS:
        if re.search(p, low):
            return "selftest"
    for p in QEMU_PATTERNS:
        if re.search(p, low):
            return "qemu"
    for p in DEMO_PATTERNS:
        if re.search(p, low):
            return "demo"
    for p in PROBE_PATTERNS:
        if re.search(p, low):
            return "probe"
    return "oneshot"


def scan() -> dict:
    """扫描 _attic 与 scripts，产出分类注册表。"""
    reg: dict[str, dict] = {}
    for base in (ATTIC, SCRIPTS):
        for f in sorted(base.iterdir()):
            if f.suffix.lower() not in (".py", ".sh") or f.name.startswith("_"):
                continue
            rel = str(f.relative_to(REPO)).replace("\\", "/")
            reg[rel] = {
                "class": classify(f.name),
                "runtime": "bash" if f.suffix == ".sh" else "python",
                "scanned": True,
            }
    return reg


# ---------------------------------------------------------------- 注册表 I/O
def load_registry() -> dict:
    if REGISTRY.exists():
        return json.loads(REGISTRY.read_text(encoding="utf-8"))
    return {"generated": None, "overrides": {}, "scripts": {}}


def save_registry(reg: dict) -> None:
    reg["generated"] = dt.datetime.now().isoformat(timespec="seconds")
    REGISTRY.write_text(
        json.dumps(reg, ensure_ascii=False, indent=1, sort_keys=True),
        encoding="utf-8",
    )


def get_entries(reg: dict) -> dict:
    """合并扫描结果与手工覆盖（覆盖优先，支持把脚本改类/注记）。"""
    merged = dict(reg.get("scripts", {}))
    for k, v in reg.get("overrides", {}).items():
        if k in merged:
            merged[k].update({**merged[k], **v})
        else:
            merged[k] = {"runtime": "python" if k.endswith(".py") else "bash",
                         "scanned": False, **v}
    return merged


def regen(reg: dict) -> None:
    old = reg
    new = {"overrides": old.get("overrides", {})}
    new["scripts"] = scan()
    # 手工覆盖过的条目不丢
    for k, v in new["overrides"].items():
        if k in new["scripts"]:
            new["scripts"][k].update(v)
    save_registry(new)
    n = len(new["scripts"])
    by = {}
    for v in new["scripts"].values():
        by[v["class"]] = by.get(v["class"], 0) + 1
    print(f"注册表重建完成：{n} 个脚本  " +
          "  ".join(f"{k}={v}" for k, v in sorted(by.items())))


# ---------------------------------------------------------------- 运行器
def run_one(rel: str, entry: dict, extra_args: list[str], log_path: Path | None,
            timeout: int | None) -> int:
    """跑单个脚本；log_path 给定则同步落盘。返回退出码。"""
    cmd = (
        ["bash", str(REPO / rel)]
        if entry["runtime"] == "bash"
        else [sys.executable, str(REPO / rel), *extra_args]
    )
    print(f"\n=== [{entry['class']}] {rel}")
    if entry["class"] == "danger":
        print("    ⚠ danger 类：可能写盘/提权/改引导——确认工单与 --yes 齐备才应到达此处")
    try:
        if log_path:
            with log_path.open("a", encoding="utf-8", errors="replace") as fh:
                fh.write(f"\n===== {rel} =====\n")
                fh.flush()
                r = subprocess.run(cmd, cwd=str(REPO), stdout=fh,
                                   stderr=subprocess.STDOUT, timeout=timeout)
            print(f"    exit={r.returncode}（日志：{log_path.name}）")
            return r.returncode
        r = subprocess.run(cmd, cwd=str(REPO), timeout=timeout)
        return r.returncode
    except subprocess.TimeoutExpired:
        print(f"    ⏱ 超时（{timeout}s），已终止——记红")
        return 124
    except FileNotFoundError as e:
        print(f"    ✗ 无法启动：{e}")
        return 127


def pick(entries: dict, classes: list[str]) -> list[str]:
    return sorted(k for k, v in entries.items() if v["class"] in classes)


# ---------------------------------------------------------------- 子命令
def cmd_fast(args) -> int:
    """宿主侧全量单测。内核 workspace 有 rust-toolchain.toml，直接 cargo test。"""
    plan = [("cargo test（kernel workspace，宿主侧全量）", ["cargo", "test"],
             REPO / "kernel")]
    if args.with_front:
        plan.append(("vitest（前端全量）", ["npm", "test", "--", "run"], REPO))
    if args.dry_run:
        for label, cmd, cwd in plan:
            print(f"[dry-run] {label}\n          cwd={cwd}  cmd={cmd}")
        return 0
    fails = 0
    for label, cmd, cwd in plan:
        print(f"\n=== [fast] {label}")
        r = subprocess.run(cmd, cwd=str(cwd))
        if r.returncode != 0:
            fails += 1
            print(f"    ✗ {label} 红（exit={r.returncode}）")
    print(f"\nfast 总结：{'全绿' if fails == 0 else f'{fails} 项红'}")
    return 0 if fails == 0 else 1


def cmd_selftest(args) -> int:
    entries = get_entries(load_registry())
    names = pick(entries, ["selftest"])
    if args.dry_run:
        print("[dry-run] selftest 队列：")
        for n in names:
            print("  ", n)
        return 0
    LOGROOT.mkdir(exist_ok=True)
    log = LOGROOT / f"selftest-{dt.datetime.now():%Y%m%d-%H%M%S}.log"
    fails, reds = 0, []
    for n in names:
        rc = run_one(n, entries[n], args.args, log, timeout=args.timeout)
        if rc != 0:
            fails += 1
            reds.append(n)
    print(f"\nselftest 总结：{len(names) - fails}/{len(names)} 绿"
          + (f"，红项：{reds}" if reds else ""))
    return 0 if fails == 0 else 1


def cmd_full(args) -> int:
    """fast + selftest + QEMU 批队列（串行，继续跑不中断，末尾总结）。"""
    entries = get_entries(load_registry())
    q = pick(entries, ["qemu"])
    if args.dry_run:
        print("[dry-run] full 队列：fast(cargo test) → selftest → QEMU 批队列：")
        for n in q:
            print("  [qemu]", n)
        return 0
    rc_fast = cmd_fast(argparse.Namespace(with_front=False, dry_run=False))
    rc_st = cmd_selftest(argparse.Namespace(args=[], timeout=args.timeout,
                                            dry_run=False))
    LOGROOT.mkdir(exist_ok=True)
    log = LOGROOT / f"qemu-queue-{dt.datetime.now():%Y%m%d-%H%M%S}.log"
    print(f"\n=== [full] QEMU 批队列（{len(q)} 项，串行继续跑，单项超时 {args.timeout}s）")
    reds = []
    for n in q:
        rc = run_one(n, entries[n], [], log, timeout=args.timeout)
        if rc != 0:
            reds.append((n, rc))
    print(f"\n===== full 总结 =====")
    print(f"fast:      {'绿' if rc_fast == 0 else '红'}")
    print(f"selftest:  {'绿' if rc_st == 0 else '红'}")
    print(f"qemu 队列: {len(q) - len(reds)}/{len(q)} 绿"
          + (f"，红项：{[n for n, _ in reds]}" if reds else "，全绿"))
    print(f"日志目录：{LOGROOT}")
    return 0 if (rc_fast == 0 and rc_st == 0 and not reds) else 1


def cmd_single(args) -> int:
    entries = get_entries(load_registry())
    target = args.name.replace("\\", "/")
    if not target.startswith("_attic/") and not target.startswith("scripts/"):
        for pref in ("_attic/", "scripts/"):
            if (REPO / pref / target).exists():
                target = pref + target
                break
    if target not in entries:
        print(f"✗ 未收编：{target}——先 `run-tests.py regen` 重扫，或检查文件名")
        return 2
    entry = entries[target]
    if entry["class"] == "danger" and not args.yes:
        print(f"⚠ {target} 是 danger 类（{CLASS_META['danger'][2]}）。")
        print("  硬件与数据安全红线：执行前必须人工确认。")
        print("  确认无误后追加 --yes 重跑；只读探针类请用 list 查同类安全替代。")
        return 3
    rc = run_one(target, entry, args.args, None, timeout=args.timeout)
    return rc


def cmd_list(args) -> int:
    entries = get_entries(load_registry())
    rows = sorted(entries.items(), key=lambda kv: (kv[1]["class"], kv[0]))
    width = max(len(k) for k in entries) if entries else 5
    for name, e in rows:
        if args.clazz and e["class"] != args.clazz:
            continue
        meta = CLASS_META.get(e["class"], ("?", "?", ""))
        flag = " *覆写" if name in reg_override_names() else ""
        print(f"{e['class']:<8} {meta[0]:<4} {meta[1]:<7} {name:<{width}}{flag}")
    by = {}
    for e in entries.values():
        by[e["class"]] = by.get(e["class"], 0) + 1
    print(f"\n共 {len(entries)} 项  " +
          "  ".join(f"{k}={v}" for k, v in sorted(by.items())))
    print("danger 类 single 需 --yes；oneshot 为存档残件不参与 fast/full。")
    return 0


_OVERRIDES: dict | None = None


def reg_override_names() -> set:
    global _OVERRIDES
    if _OVERRIDES is None:
        try:
            _OVERRIDES = set(load_registry().get("overrides", {}))
        except Exception:
            _OVERRIDES = set()
    return _OVERRIDES


def cmd_regen(_args) -> int:
    regen(load_registry())
    return 0


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser(prog="run-tests", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("fast", help="宿主侧全量单测（分钟级，日常跑）")
    p.add_argument("--with-front", action="store_true", help="附加前端 vitest")
    p.add_argument("--dry-run", action="store_true")
    p.set_defaults(fn=cmd_fast)

    p = sub.add_parser("selftest", help="跑全部已登记自检类脚本")
    p.add_argument("--timeout", type=int, default=300)
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("args", nargs="*", help="透传给脚本的附加参数")
    p.set_defaults(fn=cmd_selftest)

    p = sub.add_parser("full", help="fast + selftest + QEMU 批队列（闸门/过夜）")
    p.add_argument("--timeout", type=int, default=3600)
    p.add_argument("--dry-run", action="store_true")
    p.set_defaults(fn=cmd_full)

    p = sub.add_parser("single", help="按文件名单跑一个脚本")
    p.add_argument("name")
    p.add_argument("--yes", action="store_true", help="确认运行 danger 类")
    p.add_argument("--timeout", type=int, default=1800)
    p.add_argument("args", nargs="*", help="透传给脚本的附加参数")
    p.set_defaults(fn=cmd_single)

    p = sub.add_parser("list", help="列出收编台账")
    p.add_argument("--class", dest="clazz", choices=list(CLASS_META))
    p.set_defaults(fn=cmd_list)

    p = sub.add_parser("regen", help="重扫 _attic/scripts 重建注册表")
    p.set_defaults(fn=cmd_regen)

    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
