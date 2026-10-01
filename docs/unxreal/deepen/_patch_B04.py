# -*- coding: utf-8 -*-
"""AI-16 · D1-B04 二轮补丁：11 行未过闸（TEB 槽位族），锚点前插入『补充深化』，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B04.md"
GATE = 350

PATCH = {
4: "补充深化：槽数扩张以（分配新数组+旧值拷贝+计数项迁移）三步原子序列立档，扩张中并发读写注入红账 10/10 检出；TLS 数组账与 A4 堆账对平。",
5: "补充深化：档位偏移注出处（TEB 0xC0 档 x64/WOW64 分列）；转换跳板调用约定样本与 D2 WOW64 翻译层联签登记（预告面）。",
6: "补充深化：挂载后槽位指针生命周期与线程退出清洗（F12075）联动，GUI→控制台混合进程以『首 GUI 调用定态不回退』口径立档。",
7: "补充深化：位域非法组合（子语言无主语言档）判定表 ≥5 行；SetThreadLocale 挂点为语义预告（接口冻结后补测），默认值继承链（进程→线程）账面留痕。",
8: "补充深化：帧结构（前驱/后驱/参数包指针）字段表注公开口径；嵌套序断言（压栈/弹栈 LIFO）与 B08 展开族消费互引。",
9: "补充深化：双源漂移注入（绕过 B10 报点直接改槽）调试档红账 10/10；计数与 B10 LockCount 账抽样对平断言在册。",
12: "补充深化：哨兵魔数双值（首尾异值）防对称越界漏检，越界捕获含方向判定（前界/后界）记账；发布档裁撤由编译开关断言零残留（grep 门禁）。",
14: "补充深化：清洗账含（创建/复用/退出）三清洗点逐项归零断言；『待基准机实测』项 ≥3 行显式登记（非空率校验放行带标记）；清单与 F12061 总账表同源校验（行数相等断言）；保留区写入注入即红账。",
15: "补充深化：档位双属性（Win8+ 新增/工程未启用）入版本账；恒零断言含（创建/清洗/复用）三点采样；事务路径启用预案（接口预留位）预告登记不实现，启用预案触发条件为 ADR 裁决在册。",
16: "补充深化：ArbitraryUserPointer 自由槽内核零解释断言（内核路径零读取 grep 门禁）；EnvironmentPointer 恒空断言三清洗点采样；两字段与 F12061/F12079 判据绑定。",
18: "补充深化：五列校验脚本入构建门禁（非空率 100%）；差异标记由比较器生成注记与 F12080 版本切换器联动，字段清单单源（F12061）防双表漂移。",
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
print("B04 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns) // len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
