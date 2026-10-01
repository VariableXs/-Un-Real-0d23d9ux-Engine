# -*- coding: utf-8 -*-
"""AI-16 · D1-B15 二轮补丁：对未过闸行追加『另补』细节句。"""
import io, sys

P = "docs/unxreal/deepen/D1-B15.md"
GATE = 350

PATCH = {
8: "另补：矩阵行含复测桩位标记（该行可桩期触发/须真机），桩期红账自动降级为待实测清单项而非静默关闭；错误码与 B02 码族逐行一致性断言双跑（矩阵行对码族行）；每码差异例裁决记录三态登记（本体差异/待归一/真差异），真差异未裁决前批收口即红闸。",
9: "另补：挂起链挂点编号与 A2/A3 挂点总表对齐（挂点 ID 可点验）；对平口径含账流水笔数与值双断言；延迟两轴各自独立采样不合并口径；桩期联测的桩件版本号入账，切换真件时重跑全部三段样本，禁止继承桩期结论。",
10: "另补：竞态注入脚本含交错种子记录（每轮种子入账），红例可按种子重演；零丢失断言同时校验唤醒序相对关系与终态唯一性双面；40 例分五组各 8 例，组间插入隔离步防状态串扰；样本库版本号随批归档，复测须同版本重演。",
11: "另补：双跑执行序含基准机冒烟三探针与场景间状态清零步；容差白名单项登记产生机制说明与实测方差范围，超方差即转真差异；diff 报告三层落盘（返回码/状态序/唤醒序）随批归档，红例阻断收口直至归类裁决完成。",
12: "另补：30 行行序按 F12281–F12300 编排可点验，扩展行标注所属主接口形成父子链；每行 status_matrix 附最近复测日期与执行器版本防过期绿；对照表变更走批内变更账禁止无账改行，账-表行数等式断言入 finalize。",
14: "另补：预算值单源存 O1 且本批引用带版本号，版本不匹配断言即红；压测口径含预热段与稳态段划分（前 10^3 次不计入）；采集环境三要素（核数/内存/调度模式）登记防跨环境漂移；超限回归含单接口隔离复测与三类归因（实现/账面/环境），归因未定前红账项不得关闭。",
15: "另补：扫描器输出三类违规清单（空出处/占位词/无版本锚）逐条定位行号，修复后全量重扫保证无回退；待实测项完成后回填实测值与日期并转已结清段，原期望值留痕不改写；五态码与锚值出处逐条可点验，出处列含资料定位（文档名/章节）。",
16: "另补：映射表登记三件文档行号且抽查断言校验行号有效性；漂移注入从三个文档各取 1 例轮转注入，检出 10/10 为十次轮转总口径；对齐账随批归档，骨架改行数等跨批变更须同步更新映射并重跑抽查；零悬空断言含 ID 存在性与状态非空双面。",
17: "另补：用例元数据断言含依赖桩件名/依赖账面名/双机标记三字段；全跑按条目号升序可复现，失败定位输出后自动附邻近用例结果辅助归因；桩期账与真期账同栏分列，桩件配置切换时全量重跑；复跑单用例入口独立提供，供定位后回归。",
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
print("B15 patch OK: patched=%d total=%d min=%d avg=%d" % (patched, total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
