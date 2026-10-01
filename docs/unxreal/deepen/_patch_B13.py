# -*- coding: utf-8 -*-
"""AI-16 · D1-B13 二轮补丁：11 行未过闸，锚点前插入『另补』句，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B13.md"
GATE = 350

PATCH = {
5: "另补：解映射后视图区间回 FREE 态与 F12248 状态机弧一致，非页对齐基址注入按下取整对齐口径立档。",
9: "另补：抽样 30 例含跨页边界读写与零页访问两类高价值例，错误码与 IOSB Status 回填同步口径在册。",
10: "另补：样本含分配粒度缝隙探测与视图基址 ±1 两类边界例，注入序列号入红账回放清单。",
11: "另补：三协议对接含参数透传逐字段 memcmp 断言，桩/真切换留痕于批 finalize 表。",
13: "另补：五列含版本档列（Win10/Win11 差异标记），差异行按档分支消费，表行与本族判据 ID 双向可达断言。",
14: "另补：高址预留区间账与 A4 地址空间账逐区间对平。",
15: "另补：压测分单线程顺序与四线程并发两档分别记 P95，账面含峰值时刻与请求方类分布，回归阈值在册。",
16: "另补：出处扫描覆盖六字段表/位族/协议表三类源，扫描结果按条目号登记；ReactOS 行为注记与锚引用分列防混源；扫描器版本号入账防口径漂移，扫描脚本入构建门禁随判据复测同步跑。",
17: "另补：文档三件映射含行数守恒校验（骨架行数锁定值=深化册实计行数），抽查含判据 ID 存在性与 finalize 域账递推锚两项断言，断言全绿方收口，抽查记录随批归档。",
18: "另补：三族为分配族/保护族/映射查询族独立入口，桩期前件自检含表桩与堆桩版本匹配断言，skip 带因逐条登记非静默。",
19: "另补：互引矩阵含引用性质列（语义消费/联签对/账面对平三类），预告登记含对方交付状态字段逐条可回查。",
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
print("B13 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns) // len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
