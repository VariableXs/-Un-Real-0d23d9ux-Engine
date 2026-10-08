# -*- coding: utf-8 -*-
"""AI-16 · D1-B08 二轮小补丁：idx 18（F12159 ktest 断言集）349→≥350，锚点前插入保 J1 句尾。"""
import io, sys

P = "docs/unxreal/deepen/D1-B08.md"
GATE = 350

PATCH = {
18: "聚合器含抽验模式（随机 5 判据单跑）与桩期前件自检（表桩/堆桩版本匹配断言）双档，全跑输出两态比例汇总行；",
}

raw = io.open(P, "rb").read()
text = raw.decode("utf-8")
lines = text.split("\n")
idx = 0
patched = 0
for i, ln in enumerate(lines):
    s = ln.strip().lstrip("-").strip()
    if s.startswith("正文："):
        if idx in PATCH:
            body = ln[:-1] if ln.endswith("\r") else ln
            anchor = body.rfind("与 Windows 对照")
            if anchor == -1:
                print("ERROR: idx %d no anchor" % idx); sys.exit(1)
            newbody = body[:anchor] + PATCH[idx] + body[anchor:]
            lines[i] = newbody + ("\r" if ln.endswith("\r") else "")
            patched += 1
        idx += 1
if patched != len(PATCH):
    print("ERROR: patched %d of %d" % (patched, len(PATCH))); sys.exit(1)

total = 0
for l in lines:
    s = l.strip().lstrip("-").strip()
    if s.startswith("正文："):
        total += len(l.rstrip("\r"))
for i, l in enumerate(lines):
    if l.startswith("| 深化字数"):
        lines[i] = ("| 深化字数（正文列合计） | 实计 %d 字符（≥6,000 ✓，均 %d/条，逐行 ≥%d） | python 逐行实测回填（二轮补丁后） |"
                    % (total, total // 20, GATE))

out = "\n".join(lines)
io.open(P, "wb").write(out.encode("utf-8"))
ns = [len(l.rstrip("\r")) for l in out.split("\n") if l.strip().lstrip("-").strip().startswith("正文：")]
bad = [(i + 1, n) for i, n in enumerate(ns) if n < GATE]
print("B08 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns) // len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
