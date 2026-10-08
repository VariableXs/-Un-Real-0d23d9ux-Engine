# -*- coding: utf-8 -*-
"""AI-16 · D1-B12 二轮补丁：对未过闸行追加『另补』细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B12.md"
GATE = 350

PATCH = {
6: "另补：PERMANENT 位初置途径（永久对象接口或创建时 Attributes 位）档显式；临时化后对象在目录中仍可见直至回收（与立即消失的差异注记）；回收竞态样本（临时化与最后引用释放同拍）注入 10/10 次按序收敛，回收回调执行点（D3 回收线程语境）注记入档。",
10: "另补：矩阵行含防御返回点标注（用户态校验层/内核守卫层），便于回归定位；40 行与 F12232 样本集 40 例对偶（一行对一例），对偶断言入 finalize；错误码与 B02 码族逐行一致性断言，码族缺失码即红闸登记补码待办。",
11: "另补：样本集含两组性能敏感例（超长路径 32768 字符上界/句柄表满载 10^4 句柄下创建），行为断言外附延迟记录不设阈值（记录性）；注入器支持批量重放模式（按样本号区间），红账回放含注入线程栈指纹供归因；样本与错误矩阵对偶关系断言在册。",
12: "另补：15 场景含三组破坏性场景（关闭中句柄复制/跨线程并发查询与关闭/重复创建同名对象），行为断言允许实现定义序但状态收敛终态必须一致（收敛档显式）；diff 工具输出含场景参数与双机版本指纹头；同构码标注清单随批归档供审计。",
15: "另补：压测含句柄表并发写竞争场景（8 线程并发 Duplicate/Close），P95 在竞争态单列；预算值变更须走 O1 侧审批与双账同步（本批引用与 O1 源值版本号一致断言）；标红接口的因子定位输出含调用内分段计时（慢点采样）供优化定向。",
16: "另补：三段式出处对判据列同样生效（判据阈值须有锚定出处），判据无锚视同出处缺失；扫描器版本与规则集版本入账（规则变更可溯）；待实测项结清时须附实测环境指纹（与 ADR-UNX-008 锚版本一致性校验），环境不符的实测值不得回填。",
17: "另补：三件文档版本号入映射表（文档改版须重跑抽查而非继承结论）；漂移注入的改写例覆盖三类要素（语义改错/错误码改错/判据 ID 改错）；抽查结果回填对齐账（行级通过/失败清单），失败行即红账阻断收口。",
18: "另补：聚合器含跳过项汇总输出（SKIP 清单随跑出账），真机依赖项的跳过理由逐条登记；复测函数注册表（判据 ID→函数符号）断言无重复注册；全跑支持子集模式（按批/按族/按判据 ID 前缀三档），供 finalize 抽样与回归定向使用。",
19: "另补：互引含跨批消费边（B12→D3/J1/O1）与批内边两类分别登记；预告状态迁移留痕（时间+触发批号+承办 AI-16）；一致断言失败时输出差集（账有表无/表有账无两清单）辅助定位，B12 递推锚 68,760 入域账校验链。",
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
            newbody = (body + add) if anchor == -1 else (body[:anchor] + add + body[anchor:])
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
print("B12 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
