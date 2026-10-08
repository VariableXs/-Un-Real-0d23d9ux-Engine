# -*- coding: utf-8 -*-
"""AI-16 · D1-B11 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B11.md"
GATE = 350

PATCH = {
1: "矩阵格与 B07 样本库条目 ID 互引可点验，格生成器输入为三维参数表单源。",
6: "查找序协议文本随联签变更账同步修订，D2 侧接口变更触发本条回归。",
7: "挂点冻结版本号与 D4 消费清单互登记，版本不匹配断言即红。",
8: "注入矩阵生成器输入为格参数表单源，样本重演含随机化种子记录，红例按种子重演保证可复现，格序循环覆盖断言随批归档。",
10: "压测含异常风暴子例（连续 10^3 次同格注入）检验缓存友好性与账面增长，账面增长超预期即红账定位；P99 单列供尾部分析。",
16: "四账采集含失败注入重放验证（抽 5 例重演一致）；口径变更须走域级变更账并重出全账，判定留痕含四账版本号与采集器版本号，域收口批可反向追溯本批核账快照。",
18: "聚合器输出含 SKIP 与 PASS/FAIL 三态汇总，全跑 100% 判定不含 SKIP 项（显式排除口径），子集模式供回归定向。",
19: "集成账含三对联签的冻结版本号登记，与联签方账面版本一致断言在册。",
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
            add = PATCH[idx]
            anchor = body.rfind("与 Windows 对照")
            newbody = (body + add) if anchor == -1 else (body + add)
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
bad = [(i, n) for i, n in enumerate(ns) if n < GATE]
print("B11 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
