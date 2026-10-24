# -*- coding: utf-8 -*-
"""命令行入口。

    python -m kernelaudit                # 等价于 all，打印 Markdown 到 stdout
    python -m kernelaudit all --out x.md # 写文件
    python -m kernelaudit claims --json  # 机读
    python -m kernelaudit hazards
    python -m kernelaudit registry

退出码：0 = 无硬违规（内核路径 panic 为 0 且注册表无孤儿/重复），1 = 有硬违规。
判据层与测试层的命中不参与退出码——它们需要人工裁决，不该让 CI 红成一片。
"""

from __future__ import annotations

import argparse
import json
import os
import sys

from . import claims as claims_mod
from . import hazards as hazards_mod
from . import inventory as inv_mod
from . import registry as reg_mod
from . import report as report_mod


def _root_default() -> str:
    here = os.path.dirname(os.path.abspath(__file__))          # _tools/kernelaudit
    tools = os.path.dirname(here)                              # _tools
    return os.path.join(os.path.dirname(tools), "varix", "src")


def _root_or_die(arg: str) -> str:
    root = os.path.abspath(arg)
    if not os.path.isdir(root):
        sys.exit(f"[kernelaudit] 源码根不存在：{root}")
    return root


def cmd_claims(root: str, as_json: bool) -> int:
    bad, scanned = claims_mod.audit(root)
    if as_json:
        print(json.dumps(
            {"scanned": scanned,
             "violations": [{"file": r.rel, "kind": r.claim.kind, "count": r.count} for r in bad]},
            ensure_ascii=False, indent=2))
    else:
        print(report_mod.render_claims(bad, scanned, top_n=40))
    return 0


def cmd_hazards(root: str, as_json: bool) -> int:
    rep = hazards_mod.audit(root)
    if as_json:
        print(json.dumps({"kernel": rep.kernel, "judge": rep.judge, "test": rep.test}, indent=2))
    else:
        print(report_mod.render_hazards(rep, top_n=30))
    return 0 if rep.kernel == 0 else 1


def cmd_registry(root: str, as_json: bool) -> int:
    rep = reg_mod.audit(root)
    if as_json:
        print(json.dumps(
            {"modules": len(rep.modules), "entries": rep.table_size,
             "capacity": rep.declared_capacity, "orphans": rep.orphan_domains,
             "duplicates": rep.duplicates, "error": rep.error},
            ensure_ascii=False, indent=2))
    else:
        print(report_mod.render_registry(rep))
    return 0 if (not rep.orphan_domains and not rep.duplicates and not rep.error) else 1


def cmd_all(root: str, out: str, as_json: bool) -> int:
    inv = inv_mod.audit(root)
    haz = hazards_mod.audit(root)
    bad, scanned = claims_mod.audit(root)
    reg = reg_mod.audit(root)
    tops = inv_mod.largest_files(root, 15)

    if as_json:
        text = report_mod.to_json(inv, haz, bad, scanned, reg)
    else:
        text = report_mod.render_all(inv, haz, bad, scanned, reg, tops)

    if out:
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(text)
        print(f"[kernelaudit] 写入 {out}")
    else:
        print(text)

    hard = haz.kernel > 0 or bool(reg.orphan_domains) or bool(reg.duplicates) or bool(reg.error)
    return 1 if hard else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="kernelaudit", description="Varix 内核静态审计")
    ap.add_argument("cmd", nargs="?", default="all",
                    choices=["all", "claims", "hazards", "registry"])
    ap.add_argument("--root", default=_root_default(), help="varix/src 路径")
    ap.add_argument("--out", default="", help="Markdown 输出路径（all 子命令）")
    ap.add_argument("--json", action="store_true", help="机读输出")
    args = ap.parse_args(argv)

    root = _root_or_die(args.root)
    if args.cmd == "claims":
        return cmd_claims(root, args.json)
    if args.cmd == "hazards":
        return cmd_hazards(root, args.json)
    if args.cmd == "registry":
        return cmd_registry(root, args.json)
    return cmd_all(root, args.out, args.json)


if __name__ == "__main__":
    raise SystemExit(main())
