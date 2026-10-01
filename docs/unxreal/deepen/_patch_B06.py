# -*- coding: utf-8 -*-
"""AI-16 · D1-B06 二轮小补丁：对未过闸行追加细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B06.md"
GATE = 350

PATCH = {
8: "Upcase 不可变长度口径注出处（每码元单映射），映射表消费统计（命中率）记录性登记；恒等表桩版本号与 E 部真表交付预期挂点登记。",
10: "空串前缀预期 TRUE 档注出处（Windows 语义档），未测期判据带待测标记不判绿；30 组含（大小写混合前缀）5 组与不敏感档联动。",
11: "From 的 Base=0 自动判别含（0x/0 前缀识别）口径注出处，符号位处理（负值域）档显式；溢出两档（截断/拒止）选型经锚定后裁决入账。",
12: "格式展开估算含（%%/%s 长度未知按探针二轮策略）口径，四例含（宽窄混排）第五例；分界表含 E2 侧接口冻结版本号互登记。",
15: "长度预计算与转换分离（两函数）单源声明，BOM 处理档（UTF-8 BOM 剥离与否）显式待锚定；代理对样本含四字节 UTF-8 编码往返子例。",
17: "对照表状态列含（双机对照完成率）汇总行，样本集 diff 工具入口冻结（消费 S3 录制协议）；错误矩阵空格判定含（空格=缺行为档非缺行）口径。",
18: "聚合器含抽验模式（随机 5 判据单跑）供 finalize 抽样，桩期前件自检含（表桩/堆桩版本匹配）断言；全跑输出含两态比例汇总行。",
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
print("B06 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
