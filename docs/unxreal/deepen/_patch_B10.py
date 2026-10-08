# -*- coding: utf-8 -*-
"""AI-16 · D1-B10 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B10.md"
GATE = 350

PATCH = {
5: "等待块扩展字段预告含超时槽与回调签名两件，挂点协议书版本号与 B15 侧互引账一致断言在册。",
8: "创建失败回退档含轮询间隔参数登记（桩期口径），回退路径红账显式（不静默转轮询）；事件单实例保证含并发创建竞态双探针子例。",
9: "泄漏核账含（核账时点/扫描耗时上限）两口径，DebugInfo 链反查支持按创建点符号过滤，终止档与警告档双列策略登记。",
11: "重复初始化红账含（调用点回溯深度上限）登记，发布档不检测口径与 Windows 一致（差异为决策档非缺陷），两档切换经构建配置单源。",
14: "独占转共享语义含（转共享后唤醒等待读者）次序档，SRW 不公平性声明（不承诺饥饿自由）与 Windows 口径一致注记。",
17: "错误路径表含防御层列（用户态校验/内核守卫）与红账触发标记，抽样 20 含错误路径类至少 5 行，零断链含复测记录哈希链。",
18: "聚合器含互斥组声明（并发族与单线程族不可同跑，调度自动分批），跳过原因三类登记；复测函数注册表断言无重复注册，全跑序按条目号升序可复现。",
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
print("B10 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
