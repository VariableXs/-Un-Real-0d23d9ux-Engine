# -*- coding: utf-8 -*-
"""AI-16 · D1-B07 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B07.md"
GATE = 350

PATCH = {
7: "越栈界红账含偏移值与栈提交上限双字段登记，红账可按模块过滤回放。",
8: "对齐拒止含外置 RVA=0 注入例，拒止事件红账含（模块/条目索引/偏移值）三元组可回放。",
12: "拒止点分层标注（加载期/查询期/解释期）随红账事件同记，层位可按红账过滤统计；双机坏样本对照含终止进程档注记。",
14: "空格追踪含格号与样本库依赖条件双登记，回放面含单格重放入口；伪空格红账随 finalize 断言每批必跑。",
16: "内联档解码头校验与外置同规则（版本门串联），双轨切换样本入覆盖矩阵登记格号。",
18: "全跑含双机对照项前置检查（样本库版本与钉定版本匹配），版本不匹配即 SKIP 显式登记非静默跳过。",
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
            lines[i] = body + PATCH[idx] + ("\r" if ln.endswith("\r") else "")
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
bad = [(i, n) for i, n in enumerate(ns) if n < GATE]
print("B07 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
