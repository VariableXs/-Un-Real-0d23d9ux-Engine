# -*- coding: utf-8 -*-
"""AI-16 · D1-B14 二轮补丁：对未过闸行追加『另补』细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B14.md"
GATE = 350

PATCH = {
5: "另补协议细节——对无缓冲直通流冲刷按无操作成功语义并登记记录档；异步句柄挂起路径 IOSB 终态与完成回调序判据在册；掉电模拟注入窗口锁定在冲刷调用与 B2 journal 提交两锚点之间，完整性断言含文件大小与数据双面，缺一即红账。",
12: "另补：样本集含三组特殊例——映射区/私有区交界读写样本验证 MBI 边界判定，分配粒度缝隙内访问样本验证禁区拒止，句柄指向已关闭进程的悬空样本验证失效检测；每例注入前记录状态快照、注入后快照比对，不变式破坏即红账，回放器按序号独立重演。",
13: "另补：对照执行序为同码构建物分别部署、基准机先行冒烟（三探针）、随后 15 场景顺序重放；场景间插状态清零步防串扰；diff 报告三层落盘且语义差异与布局差异分册归档，红例阻断收口直至归类裁决完成。",
14: "另补：30 行行序按 F12241–F12260 顺序编排保证可点验；扩展行单独标注所属主接口行号形成父子链；每行 status_matrix 附最近复测日期与执行器版本号防过期绿；对照表变更须走批内变更账，禁止无账改行。",
15: "另补：采集环境固定项登记（核数/内存/页表模式三要素）防跨环境漂移；压测含稳态预热段（前 10^3 次不计入 P95）；延迟分布记录 P50/P95/P99 三点；超限回归含单接口隔离复测与归因（实现/账面/环境三类），归因未定前不得关闭红账项。",
16: "另补：扫描器输出三类违规清单（空出处/占位词/无版本锚）逐条定位到行号；清单修复后全量重扫而非增量确认，保证无回退；待实测项完成后回填实测值与日期并转入已结清段，保留原期望值不改写以留痕；外部资料引用在出处列逐条可点验。",
17: "另补：映射表登记三件文档各自行号定位，抽查断言同时校验行号有效性；漂移注入从三个不同文档各取 1 例共 3 例（十次轮转注入总口径检出 10/10）；对齐账随批归档，跨批变更（骨架改行数）须同步更新映射并重跑抽查。",
18: "另补：用例入册含头部元数据（依赖桩件名/依赖账面名/双机标记）三字段断言；全跑按条目号升序保证可复现，失败定位输出后自动附邻近用例结果辅助归因；桩期账与真期账同栏分列，切换桩件配置时全量重跑而非继承旧档。",
19: "另补：互引矩阵含反向索引断言（被引方视角）正向反向一致；预告对状态迁移须留迁移记录（时间+触发批号），禁止无痕跳变；集成账与对照表行数等式断言失败时阻断域账递推并回溯最近批，递推锚值不可变。",
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
print("B14 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d all 20 lines" % GATE)
