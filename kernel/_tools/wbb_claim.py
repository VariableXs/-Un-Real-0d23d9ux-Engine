#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""wbb_claim.py — AI-WBB-1022 第四批 200 单领单写回（流式逐卡块）
领单范围：F0600,F0626~F0644,F0646~F0699（74）+ F0800,F0820~F0899（81）+ F0900~F0944（45）
动作（仅限 状态=待领 且 领取人为空 的目标卡）：
  - 状态: 待领 → 已领
  - 领取人: (空) → AI-WBB-1022
"""
import io, os, sys

TB = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixTaskOps\VTaskBoard\taskboard.md"
OWNER = "AI-WBB-1022"

TARGETS = set([600] + list(range(626, 645)) + list(range(646, 700))
              + [800] + list(range(820, 900)) + list(range(900, 945)))

DRY = "--dry" in sys.argv

def flush(block, fout, done):
    if not block:
        return
    head_line = block[0]
    if not head_line.startswith("## [VE-F"):
        fout.writelines(block)
        return
    head = head_line[3:].split("]")[0].strip().lstrip("[").strip()
    num = head.split("-F")[1] if "-F" in head else ""
    if not (num.isdigit() and int(num) in TARGETS):
        fout.writelines(block)
        return
    # 目标卡：检查是否 待领+无主
    st_idx = own_idx = None
    for i, l in enumerate(block):
        s = l.rstrip("\n")
        if s == "- 状态: 待领":
            st_idx = i
        elif s == "- 领取人: ":
            own_idx = i
    if st_idx is None or own_idx is None:
        fout.writelines(block)  # 已被他人领/状态异常，不动
        done["跳过"] += 1
        return
    block[st_idx] = "- 状态: 已领\n"
    block[own_idx] = "- 领取人: %s\n" % OWNER
    done["领单"] += 1
    fout.writelines(block)

def main():
    tmp = TB + ".tmp_claim"
    block = []
    done = {"领单": 0, "跳过": 0}
    with io.open(TB, "r", encoding="utf-8", errors="replace") as fin, \
         io.open(tmp, "w", encoding="utf-8", newline="") as fout:
        for line in fin:
            if line.startswith("## ["):
                flush(block, fout, done)
                block = [line]
            else:
                block.append(line)
        flush(block, fout, done)
    if DRY:
        os.remove(tmp)
        print("DRY:", done)
    else:
        os.replace(tmp, TB)
        print("WROTE:", done)
        print("SIZE:", os.path.getsize(TB))

if __name__ == "__main__":
    main()
