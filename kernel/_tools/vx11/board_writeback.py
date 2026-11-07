#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""任务板回写：按 id 清单把「已领」改成「已完成」并写入结果/时间。

用法: python board_writeback.py results.json [--apply]
results.json: {"9282": "unx... .rs 1100 行 ...", ...}
"""
import io
import json
import os
import re
import sys
import time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BOARDS = [
    os.path.join(ROOT, r"VarixTaskOps\VTaskBoard\taskboard.md"),
    os.path.join(ROOT, r"dist\taskboard.md"),
]
OWNER = "AI-VX11-1105"


def patch_block(block, result, stamp):
    lines = block.split("\n")
    for i, ln in enumerate(lines):
        if ln.startswith("- 状态:"):
            lines[i] = "- 状态: 已完成"
        elif ln.startswith("- 领取人:"):
            lines[i] = "- 领取人: " + OWNER
        elif ln.startswith("- 结果:"):
            lines[i] = "- 结果: " + result
        elif ln.startswith("- 时间:"):
            lines[i] = "- 时间: " + stamp
    return "\n".join(lines)


def patch_board(path, results, stamp, apply):
    src = io.open(path, encoding="utf-8").read()
    out = src
    hits, miss = 0, []
    for nid, result in results.items():
        tag = "## [UNX-F%s]" % nid
        start = out.find(tag)
        if start < 0:
            miss.append(nid)
            continue
        nxt = out.find("\n## [", start + 1)
        end = len(out) if nxt < 0 else nxt + 1
        block = out[start:end]
        if "- 状态: 已完成" in block and ("- 结果: " + result) in block:
            hits += 1
            continue
        new = patch_block(block, result, stamp)
        out = out[:start] + new + out[end:]
        hits += 1
    if apply and out != src:
        bak = path + ".bak-%s" % time.strftime("%m%d_%H%M%S")
        io.open(bak, "w", encoding="utf-8", newline="").write(src)
        io.open(path, "w", encoding="utf-8", newline="").write(out)
        print("%s patched=%d backup=%s" % (os.path.basename(path), hits, os.path.basename(bak)))
    else:
        print("%s dryrun hits=%d miss=%s" % (os.path.basename(path), hits, ",".join(miss)))
    return miss


def main():
    apply = "--apply" in sys.argv
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    results = json.load(io.open(args[0], encoding="utf-8"))
    results = {str(k): v for k, v in results.items()}
    stamp = time.strftime("%m-%d %H:%M")
    for p in BOARDS:
        miss = patch_board(p, results, stamp, apply)
        if miss:
            print("MISSING CARDS in %s: %s" % (p, ",".join(miss)))


if __name__ == "__main__":
    main()
