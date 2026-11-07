#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 plan 中 9282~9401 的模块注册进 mod.rs：两处注册表 + 一处 pub mod 列表。

锚点（唯一）：
  1) ("UNX-F9281", unx9281_charter_proc::run_unx9281_checks as fn() -> CheckSet),
  2) pub mod unx9281_charter_proc;
"""
import io
import json
import os
import sys
import time

KERNEL = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel"
MODS = os.path.join(KERNEL, r"varix\src\unxstar")
MODRS = os.path.join(MODS, "mod.rs")
PLAN = os.path.join(KERNEL, r"_tools\vx11\plan.json")

REG_ANCHOR = '("UNX-F9281", unx9281_charter_proc::run_unx9281_checks as fn() -> CheckSet),'
MOD_ANCHOR = 'pub mod unx9281_charter_proc;'


def load_cards():
    plan = json.load(io.open(PLAN, encoding="utf-8"))
    cards = [c for c in plan if 9282 <= c["id"] <= 9401]
    out = []
    for c in cards:
        p = os.path.join(MODS, c["stem"] + ".rs")
        if not os.path.exists(p):
            print("SKIP missing %d %s" % (c["id"], c["stem"]))
            continue
        out.append(c)
    return out


def main():
    apply = "--apply" in sys.argv
    cards = load_cards()
    src = io.open(MODRS, encoding="utf-8").read()

    reg_lines, mod_lines = [], []
    for c in cards:
        tid = "UNX-F%d" % c["id"]
        tuple_line = '        ("%s", %s::run_unx%d_checks as fn() -> CheckSet),' % (tid, c["stem"], c["id"])
        mod_line = "pub mod %s;" % c["stem"]
        if tid not in src:
            reg_lines.append(tuple_line)
        if mod_line not in src:
            mod_lines.append(mod_line)
    # cards 已按 id 升序，行序即注册序
    reg_block = ("\n".join(reg_lines) + "\n") if reg_lines else ""
    mod_block = ("\n".join(mod_lines) + "\n") if mod_lines else ""

    print("cards=%d to_register=%d to_mod=%d" % (len(cards), len(reg_lines), len(mod_lines)))
    if not apply:
        return

    if src.count(REG_ANCHOR) != 2:
        print("ABORT: REG_ANCHOR count=%d" % src.count(REG_ANCHOR))
        return
    if src.count(MOD_ANCHOR) != 1:
        print("ABORT: MOD_ANCHOR count=%d" % src.count(MOD_ANCHOR))
        return

    if reg_block:
        src = src.replace(REG_ANCHOR, REG_ANCHOR + "\n" + reg_block.rstrip("\n"))
    if mod_block:
        src = src.replace(MOD_ANCHOR, MOD_ANCHOR + "\n" + mod_block.rstrip("\n"))

    bak = MODRS + ".bak-%s" % time.strftime("%m%d_%H%M%S")
    io.open(bak, "w", encoding="utf-8", newline="").write(io.open(MODRS, encoding="utf-8").read())
    io.open(MODRS, "w", encoding="utf-8", newline="").write(src)
    print("applied; backup=%s" % bak)


if __name__ == "__main__":
    main()
