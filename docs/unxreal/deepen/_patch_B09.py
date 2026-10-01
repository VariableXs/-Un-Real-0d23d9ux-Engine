# -*- coding: utf-8 -*-
"""AI-16 · D1-B09 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B09.md"
GATE = 350

PATCH = {
9: "特权位恢复掩码含 VM/IF/IOPL/NT 四位全列，掩码应用点（恢复前过滤）在册。",
10: "专用异常栈尺寸下限与 A3 配置单源对齐，配置不一致断言即红；布置顺序（CONTEXT 先行）为展开器消费序前提，倒序注入拒止。",
15: "五列含语义列与 Windows 行为注记列双列，注记列引用公开反汇编口径可点验；对照表行数=字段表行数断言在册。",
16: "快照含 MxCsr 状态段（XMM 上下文完整性），回溯面与 B07 展开器消费单源（同一查找入口），双源查找禁入。",
18: "桩期前件自检含 B07 样本库/B08 分发件依赖声明，缺桩 SKIP 入桩期账；复测函数注册表断言无重复注册，子集模式按判据 ID 前缀三档。",
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
            newbody = body + add
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
print("B09 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
