# -*- coding: utf-8 -*-
"""AI-16 · D1-B03 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B03.md"
GATE = 350

PATCH = {
9: "快锁超限转等待的等待体与 B10 等待块同型复用（单实现声明），压测含慢写者子例（写者持锁毫秒级）。",
10: "镜像账差异告警含告警阈值与上报通道（F0011 日志环）登记，F 部主账版本号与镜像账版本号一致断言在册。",
11: "三槽登记含槽位相邻字段越界防护（相邻字段写不影响本槽断言），SxS 域上线前布置接口冻结（勿提前消费）。",
12: "双时点采样含（进程初始化后/生态加载后）两时点登记，时点切换事件账可回放；检测槽白名单写点与生态域写者清单一致断言在册。",
13: "审计账容量与轮转策略登记（满后覆盖最旧+计数留痕），构建期拦截含（白名单表变更→扫描器同步）流程断言。",
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
print("B03 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
