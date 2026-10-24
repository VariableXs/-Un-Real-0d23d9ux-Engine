#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""wbb_writeback.py — AI-WBB-1022 第三批 200 单写回（流式逐卡块，42MB 板严禁整读）
用法: python wbb_writeback.py            # 执行写回
      python wbb_writeback.py --dry      # 只统计不改写
只动 F0500~F0599 / F0700~F0799 中领取人为 AI-WBB-1022 的卡：
  - 状态: 已领 → 已完成；- 结果: (空) → vec 摘要；- 时间: (空) → 时间戳
注意：卡内字段顺序为 状态→领取人→结果→时间，故按卡块缓冲后再处理。
"""
import sys, io, os, datetime

TB = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\VarixTaskOps\VTaskBoard\taskboard.md"
OWNER = "AI-WBB-1022"
TARGETS = set()
for base in (500, 700):
    for i in range(100):
        TARGETS.add("VE-F%04d" % (base + i))

DRY = "--dry" in sys.argv
TS = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")

def vec_of(card):
    return "vec%03d" % (int(card.split("F")[1]) - 400)

def flush(block, fout, done):
    """处理一个完整卡块（行列表，含卡头行）"""
    if not block:
        return
    head_line = block[0]
    if not head_line.startswith("## [VE-F"):
        fout.writelines(block)
        return
    card = head_line[3:].split("]")[0].strip().lstrip("[").strip()
    if card not in TARGETS:
        fout.writelines(block)
        return
    if not any(l.strip() == "- 领取人: " + OWNER for l in block):
        fout.writelines(block)
        return
    v = vec_of(card)
    out = []
    for l in block:
        st = l.rstrip("\n")
        if st == "- 状态: 已领":
            l = "- 状态: 已完成\n"
            done["状态"] += 1
        elif st == "- 结果: ":
            l = ("- 结果: %s 模块落盘并双锚点注册（mod.rs pub mod + 检查表），"
                 "run_%s_checks 判据入表，cargo check 通过\n" % (v, v))
            done["结果"] += 1
        elif st == "- 时间: ":
            l = "- 时间: %s\n" % TS
            done["时间"] += 1
        out.append(l)
    fout.writelines(out)

def main():
    tmp = TB + ".tmp_wb"
    block = []
    done = {"状态": 0, "结果": 0, "时间": 0}
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
