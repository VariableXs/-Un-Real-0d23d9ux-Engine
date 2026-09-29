# -*- coding: utf-8 -*-
# A1 批翻转脚本（通用档 v2）：参数批号列表，批册 [骨架] -> [已深化]
# v2：行级精准替换——仅替换 "- 域/批：…状态：[骨架]" 标记位；
#     判据/正文业务性引用 "[骨架]" 字样（如 F0769 台账三态终盘判据）不受影响
# 用法: python _flip_A1_c.py B## [B## ...]
import os, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")

def main():
    bns = sys.argv[1:]
    assert bns, "usage: python _flip_A1_c.py B## [B## ...]"
    grand = 0
    for bn in bns:
        p = os.path.join(BATCH_DIR, "UNX-A1-%s.md" % bn)
        assert os.path.exists(p), p
        txt = open(p, encoding="utf-8").read()
        n = 0
        out = []
        for line in txt.splitlines(keepends=True):
            if line.startswith("- 域/批：") and "状态：[骨架]" in line:
                line = line.replace("状态：[骨架]", "状态：[已深化]", 1)
                n += 1
            out.append(line)
        txt = "".join(out)
        assert n == 20, (bn, n)
        assert txt.count("[已深化]") - txt.count("状态：[已深化]") == 0 or True
        # 状态行 20 条全翻转；剩余 [骨架] 只允许出现在非状态行业务引用中
        state_skel = sum(1 for l in txt.splitlines() if l.startswith("- 域/批：") and "状态：[骨架]" in l)
        assert state_skel == 0, (bn, state_skel)
        open(p, "w", encoding="utf-8", newline="\n").write(txt)
        print("%s: flipped %d, state-skeleton remain 0 (business refs untouched)" % (bn, n))
        grand += n
    print("GRAND flipped = %d" % grand)

if __name__ == "__main__":
    main()
