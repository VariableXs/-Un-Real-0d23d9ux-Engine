# -*- coding: utf-8 -*-
# A1 批翻转脚本：批册 [骨架] -> [已深化]，B33 仅前 3 条（F0641-F0643）
import os, sys, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")

BATCHES = ["B19", "B20", "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28", "B29", "B30", "B31", "B32"]
PARTIAL = {"B33": ["F0641", "F0642", "F0643"]}

def main():
    grand = 0
    for bn in BATCHES + list(PARTIAL.keys()):
        p = os.path.join(BATCH_DIR, "UNX-A1-%s.md" % bn)
        txt = open(p, encoding="utf-8").read()
        if bn in PARTIAL:
            ids = PARTIAL[bn]
            n = 0
            for fid in ids:
                tag = "### UNX-%s · " % fid
                out, pend = [], False
                hit = False
                for line in txt.splitlines(keepends=True):
                    if line.startswith(tag):
                        pend, hit = True, True
                        out.append(line)
                        continue
                    if pend and line.startswith("- 域/批："):
                        assert "[骨架]" in line, (bn, fid)
                        line = line.replace("[骨架]", "[已深化]", 1)
                        n += 1
                        pend = False
                    out.append(line)
                assert hit and not pend, (bn, fid, hit, pend)
                txt = "".join(out)
            remain = txt.count("[骨架]")
            assert remain == 20 - len(ids), (bn, remain)
            print("%s: flipped %d, skeleton remain %d" % (bn, n, remain))
        else:
            before = txt.count("[骨架]")
            assert before == 20, (bn, before)
            txt = txt.replace("[骨架]", "[已深化]")
            assert txt.count("[已深化]") == 20, bn
            print("%s: flipped 20, skeleton remain 0" % bn)
        grand += (len(PARTIAL[bn]) if bn in PARTIAL else 20)
        open(p, "w", encoding="utf-8", newline="\n").write(txt)
    print("GRAND flipped = %d (expect 283 = 14x20 + 3)" % grand)
    assert grand == 283

if __name__ == "__main__":
    main()
