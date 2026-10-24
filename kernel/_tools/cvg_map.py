#!/usr/bin/env python3
"""cvg_map.py — 扫描源码，建立施工书编号 → 落点文件 的覆盖映射。

用法: python cvg_map.py <册前缀> <lo> <hi>
例:   python cvg_map.py CGPU 14 113
输出: 每个编号的落点文件列表（空 = 无任何代码引用）。
"""
import collections
import os
import re
import sys

SRC = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    pre = sys.argv[1]
    lo = int(sys.argv[2])
    hi = int(sys.argv[3])
    pat = re.compile(pre + r"-F(\d{4})")
    m = collections.defaultdict(list)
    for root, _dirs, files in os.walk(SRC):
        if os.sep + "target" in root:
            continue
        for f in files:
            if not f.endswith(".rs"):
                continue
            p = os.path.join(root, f)
            try:
                t = open(p, encoding="utf-8", errors="replace").read()
            except OSError:
                continue
            for x in set(pat.findall(t)):
                m[int(x)].append(os.path.relpath(p, SRC).replace(os.sep, "/"))
    have = [i for i in range(lo, hi + 1) if i in m]
    print("range %d..%d covered: %d/%d" % (lo, hi, len(have), hi - lo + 1))
    for i in range(lo, hi + 1):
        locs = m.get(i) or []
        flag = "OK " if locs else "-- "
        print("%sF%04d %s" % (flag, i, ", ".join(sorted(locs)[:3])))


if __name__ == "__main__":
    main()
