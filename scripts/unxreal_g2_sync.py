#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""UNX-G2 首产段 B01–B15 四处同步：汇编册 / 总纲 §7.3-G2 / 根台账 / handoff.json。
纯追加+定点小改，零改写他会话内容（append-only 纪律）。"""
import os, io, re, json, datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
ASM = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
MASTER = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
HANDOFF = os.path.join(ROOT, "docs", "unxreal", "handoff.json")

def rd(p): return io.open(p, encoding="utf-8").read()
def wr(p, s): io.open(p, "w", encoding="utf-8", newline="\n").write(s)

BATCH_RE = re.compile(r"^### UNX-F(\d+) · (.+)$")
META_RE = re.compile(r"^- 域/批：G2/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：UNX-F(\d+)-J1 (.+)$")

def parse_batch(path):
    title = ""
    entries = []
    for line in rd(path).splitlines():
        if line.startswith("# UNX-G2-B"):
            title = line[2:].split("（")[0].strip()
        m = BATCH_RE.match(line)
        if m:
            entries.append({"fid": int(m.group(1)), "name": m.group(2)})
            continue
        m = META_RE.match(line)
        if m:
            entries[-1].update(batch=int(m.group(1)), rows=int(m.group(2)), status=m.group(3), crit=m.group(5))
    assert all("crit" in e for e in entries) and len(entries) == 20, path
    return title, entries

BATCHES = [parse_batch(os.path.join(BATCH_DIR, f"UNX-G2-B{n:02d}.md")) for n in range(1, 16)]

# ---------- 1. 汇编册 ----------
asm = rd(ASM)
assert "dom-G2" not in asm, "assembly already has dom-G2"
E5_ROW = "| [UNX-E5](#dom-E5) | 反检测与疑难修复库 | AI-25 | 40（B01–B40 满账） | 800 | F19201–F20000 | 240,000 | 800 / 0 |"
G2_ROW = "| [UNX-G2](#dom-G2) | Mesa/Gallium 与 Vulkan | AI-32 | 15（B01–B15 首产段） | 300 | F24801–F25100 | 90,000 | 0 / 300 |"
assert E5_ROW in asm
asm = asm.replace(E5_ROW, E5_ROW + "\n" + G2_ROW, 1)

OLD_SUM = "| **合计（25 域 + D4 增补卷三 + D4 增补卷四 + E4 增补卷一/二/三/四/五/六/七）** | — | — | **1163** | **23260** | — | **7,038,240** | **10640 / 9520** |"
NEW_SUM = "| **合计（26 域（含 G2 首产段）+ D4 增补卷三/四 + E4 增补卷一/二/三/四/五/六/七）** | — | — | **1178** | **23560** | — | **7,128,240** | **10640 / 9820** |"
assert OLD_SUM in asm
asm = asm.replace(OLD_SUM, NEW_SUM, 1)

MAP_ANCH = "- **部E · 运行库与网络生态**"
MAP_LINE = "- **部G · GPU 图形栈**（知识层六：GPU 图形栈（本册现收 G2 首产段，G1/G3/G4/G5 批册未立随认领增补））：[UNX-G2 Mesa/Gallium 与 Vulkan](#dom-G2)\n"
idx = asm.index(MAP_ANCH)
eol = asm.index("\n", idx) + 1
asm = asm[:eol] + MAP_LINE + asm[eol:]

GAP_ANCH = "- 工程公理口径：80 域 × 800 条 = 64,000 条；"
GAP_LINE = "- UNX-G2：AI-32 承办（波 13 主力域首产段立账）——本快照收录 B01–B15 十五批 300 条/90,000 行（骨架态，Gallium 嫁接地基/iris/crocus+llvmpipe/GL 面与 state tracker/RADV/NVK/lavapipe/piglit/VK-GL-CTS/跟随制度与首产段收口十五主题；生成器 scripts/unxreal_g2_skeleton_gen.py，校验器 scripts/unxreal_g2_skeleton_check.py 五查 ALL PASS；B16–B40 待领余 150,000 行）。\n"
asm = asm.replace(GAP_ANCH, GAP_LINE + GAP_ANCH, 1)

dom = ["\n---\n", '<a id="part-G"></a>\n', "\n## 部G · GPU 图形栈\n",
       "\n> 知识层六：GPU 图形栈（KMD/Mesa/DXVK/vkd3d/显示输出——本册现收 UNX-G2 首产段 B01–B15，G1/G3/G4/G5 随认领增补）\n",
       '\n<a id="dom-G2"></a>\n', "\n### UNX-G2 · Mesa/Gallium 与 Vulkan\n",
       "\n> 域档｜承办 AI-32｜批册 15（B01–B15 首产段）｜条目 300｜F24801–F25100｜行数合计 90,000｜已深化 0 / 骨架 300",
       "\n> 域注：G2 嫁接 Mesa（iris Intel Gen9+/crocus Gen4–Gen7/llvmpipe/RADV AMD RDNA/NVK/lavapipe）+ piglit + VK-GL-CTS 账本体系，季度跟随，风险级高，兜底 lavapipe；CTS 绿账为 G3/G4 空转警戒线（锚 1–3 见《项目全景图》UNX-G2 任务书）；winsys 唯一内核接触面走 G1 冻结联签三锚；软渲染标注四十六字诀「缺件如实标」全域生效；B16–B40 待领（余 150,000 行）。\n"]
for n, (title, entries) in enumerate(BATCHES, 1):
    dom.append(f"\n#### UNX-G2-B{n:02d} · {title}（F{entries[0]['fid']}–F{entries[-1]['fid']} · 20 条）\n")
    dom.append("\n> AI-32 承办｜批内 20 条 × 6,000 行｜骨架态（深化收口见 deepen/）\n")
    dom.append("\n| 编号 | 功能条目 | 行数 | 状态 | 判据 |")
    dom.append("|---|---|---|---|---|")
    for e in entries:
        dom.append(f"| UNX-F{e['fid']} | {e['name']} | {e['rows']} | 骨架 | UNX-F{e['fid']}-J1 {e['crit']} |")
asm = asm.rstrip("\n") + "\n" + "\n".join(dom) + "\n"
wr(ASM, asm)
print("assembly ok")

# ---------- 2. 总纲 §7.3-G2 ----------
m = rd(MASTER)
assert "7.3-G2" not in m
rows = []
for n in range(1, 41):
    lo = 24801 + (n - 1) * 20; hi = lo + 19
    if n <= 15:
        title = BATCHES[n - 1][0]
        rows.append(f"| UNX-G2-B{n:02d} | F{lo}–F{hi} | 20 | [骨架] | 0（骨架已立，6,000 行预算锁定，批册 batches/UNX-G2-B{n:02d}.md）（主题框架：{title}） | AI-32 |")
    else:
        rows.append(f"| UNX-G2-B{n:02d} | F{lo}–F{hi} | 20 | [未动] | — | 待领 |")
table = "\n".join(rows)
sec = f"""
#### 7.3-G2 域 G2 台账正文（40 批全表 · AI-32 认领扩表 · §7.6 规则一次建入）

> 扩表校验（§7.6 规则 2 三项机械校验）：ID 区间 F24801–F25600 与 §2.2 分配总表一致 ✓（《AI分工完成图》v2.0 AI-32 任务书同口径，波 13 主力域，全局关键路径 #5）；40 批 × 20 条 = 800 条 ✓；批区间首尾相接无空洞无重叠 ✓（校验器回算留痕：首产段 15 批 × 20 条 = 300 条，F24801–F25100 连续唯一，批批求和 6,000 与批头登记一致，总 90,000——scripts/unxreal_g2_skeleton_check.py 五查 ALL PASS exit=0）。主题框架承《AI分工完成图》AI-32 任务书与《项目全景图》UNX-G2 节条目构成：B01–B03 Gallium 嫁接地基与 winsys×G1 联签（三锚）、B04–B05 iris Gen9+ 上栈与深化、B06 crocus+llvmpipe 扩展位与 GL 软渲染兜底、B07 GL state tracker 与 GL 4.6 面、B08–B09 RADV 上栈与深化、B10 NVK 分档、B11 lavapipe Vulkan 兜底、B12 piglit 账本体系、B13 VK-GL-CTS 账本与归因三分法、B14 季度跟随制度与降级演练、B15 首产段收口；B16–B40 待领（C 型收官批型配比在尾段预留）。**嫁接与跟随声明**：全嫁接档（风险级高）——Mesa/piglit/VK-GL-CTS 只跟随钉版（钉版三件：版本/commit/哈希），改动只落 winsys/适配器白名单层，季度换版走跟随账本、通过率只升不降（降即冻结换版并触发 AI-94 风险复评）；兜底 lavapipe/llvmpipe 为保险丝非耻辱柱，软渲染四十六字诀「缺件如实标」全域判据化。**防重四范围已执行**：kernel 命中仅 wingl.rs/path3.rs/lib.rs/quality.rs 的 llvmpipe 注释引用（E1 域借力清单形态非实装，本域立嫁接本体，边界对账条 F25100 显式声明）；docs/VE、docs/START、已 finalize 批次零 G2 ID 占用。**上游依赖**：G1（AI-31）winsys 联签三锚未冻结——按 Schema 先行 + fake 对接（R-G2-001），冻结后联签回归；F2 GLX/EGL 桥（AI-27）、G3/G4 下游（AI-33/34）消费锚槽位预置。**红线适用**：无引导设施红线与硬件数据安全红线触发条目；winsys 全部内核接触走 G1 冻结接口零旁路；真机判据一律登记 R-G2-002 随闸门补测（双轨产线零 QEMU 零实机写）。判据主轴：CTS 绿账（piglit/VK-GL-CTS 账本化通过率）+ 软渲染兜底演练账 + 显存泄漏零漂移账。

| 批次 | ID 区间 | 条数 | 三态 | 深化字数 | 承接会话 |
|---|---|---|---|---|---|
{table}

**域 G2 台账小结**（写锁登记时点）：40 批 · 800 条 · 域账 240,000 行。B01–B15 首产段立账（AI-32 承办，Variable 明令本会话 300 项新功能：B01–B03 Gallium 嫁接地基/winsys 三面/winsys×G1 联签 60 条 + B04–B05 iris Gen9+ 40 条 + B06 crocus/llvmpipe 20 条 + B07 GL state tracker 20 条 + B08–B09 RADV 40 条 + B10 NVK 20 条 + B11 lavapipe 兜底 20 条 + B12–B13 piglit/VK-GL-CTS 账本 40 条 + B14 跟随制度与演练 20 条 + B15 首产段收口 20 条，行数预算合计 90,000）· B16–B40 待领（余 150,000 行）。闭账物预告：CTS 首轮全绿账（E 级口径：账本豁免显式列出且经 AI-96 复核）为波 13 闭账物，AI-32 主办；里程碑锚 1–3（winsys 联签冻结/piglit+CTS 首轮开跑/CTS 全绿账+lavapipe 兜底演练）按任务书锚点推进。

> 修订记录（AI-32 会话 2026-10 · 波13 首产段）：§7.6 规则一次建入域 G2 全表 40 批；B01–B15 认领写锁（承接会话 AI-32）；批册 batches/UNX-G2-B01..B15.md 十五件落盘（F24801–F25100 连续唯一、批批 6,000 守恒、判据 300 枚唯一）；生成器 scripts/unxreal_g2_skeleton_gen.py 与校验器 scripts/unxreal_g2_skeleton_check.py 入库；汇编册部G 域节纯追加；根台账 §三/§四、handoff G2 块最小 patch 同步；open_risks：R-G2-001（G1 联签三锚未冻结，Schema 先行+fake）/ R-G2-002（真机判据随闸门补测，双轨产线零 QEMU）/ R-G2-003（Mesa 季度换版风险级高，跟随账本降级冻结机制就位）。
"""
wr(MASTER, m.rstrip("\n") + "\n" + sec)
print("master ok")

# ---------- 3. 根台账 ----------
led = rd(LEDGER)
ANCHOR_ROW = "| 其余 75 域 | 待领 |"
G2_LEDGER_ROW = "| UNX-G2 Mesa/Gallium 与 Vulkan | AI-32 | 0 | B01–B15（15 批 300 条首产段） | 0 | 90,000 / 240,000（B01–B15 骨架 15 批 × 6,000，脚本实核零偏离） | 波 13 窗首产段立账：Gallium 嫁接地基与 winsys×G1 联签三锚（B01–B03）+ iris Gen9+（B04–B05）+ crocus/llvmpipe 兜底（B06）+ GL state tracker（B07）+ RADV（B08–B09）+ NVK（B10）+ lavapipe（B11）+ piglit/VK-GL-CTS 账本（B12–B13）+ 季度跟随与降级演练（B14）+ 首产段收口（B15）；生成器 scripts/unxreal_g2_skeleton_gen.py、校验器 scripts/unxreal_g2_skeleton_check.py 五查 ALL PASS；B16–B40 待领（余 150,000 行） |"
assert ANCHOR_ROW in led
led = led.replace(ANCHOR_ROW, G2_LEDGER_ROW + "\n" + ANCHOR_ROW, 1)
log = f"""
### 会话 2026-波13-M01 · AI-32（G2 域认领扩表 + B01–B15 首产段骨架 300 条）
- **冷启动对账**：git + handoff.json + 总纲 §7.3 三方对账；读入四份源册 AI-32 任务书全文（UNX-G2：Mesa/Gallium 与 Vulkan，F24801–F25600，40 批，波 13 窗，上游 AI-31 G1，下游 G3/G4，闭账物 CTS 绿账）与《项目全景图》UNX-G2 节全文、§6 施工协议、§7.6 扩表规则。工作区他会话产物不越权代管。
- **认领写锁（§7.6 扩表规则）**：总纲 §7.3-G2 域台账 40 批全表一次建入（三项机械校验通过留痕：ID 区间 F24801–F25600 ✓、40×20=800 ✓、批区间首尾相接 ✓），B01–B15 承接会话写锁 AI-32，B16–B40 待领。
- **本会话 300 项新功能**（Variable 明令口径：一次对话 300 项；全部围绕 Varix 内核锚定：winsys 唯一内核接触面、G1 联签三锚 Schema 先行、软渲染兜底四十六字诀「缺件如实标」、CTS 账本化归因三分法）：
  - B01–B03 Gallium 嫁接地基 60 条（F24801–F24860）：Mesa 版本钉定与季度跟随账本/三面分层/winsys 接触面穿透防线/pipe_screen·pipe_context·pipe_resource 语义面/winsys×G1 联签三锚（dumb buffer/execbuf/fence，Schema 先行+fake，R-G2-001）/显存账本消费与 10^5 次零漂移/主驱动人为失效软渲染接管降级账；
  - B04–B07 GL 面 80 条（F24861–F24940）：iris Gen9+ 探针/编译/batch/格式表/呈现与 F1 挂点 + crocus Gen4–Gen7 T2 扩展位 + llvmpipe GL 软渲染兜底与性能基线 + GL state tracker 与 GL 4.6 能力面（零虚报扩展）；
  - B08–B11 Vulkan 面 80 条（F24941–F25020）：RADV RDNA 上栈/ACO 编译/时间轴信号量/descriptor indexing 深化 + NVK T1-C 分档（能力降档如实标注）+ lavapipe 全域兜底（零 GPU 自举/虚拟机档/双软引擎资源预算）；
  - B12–B15 验收与收口 80 条（F25041–F25100）：piglit 账本体系（T1-A 判据来源、豁免 ≤5%、假全绿五模式防护）+ VK-GL-CTS 账本与归因三分法（T1-B，E 级全绿账框架、AI-96 复核槽位）+ 季度换版 SOP/通过率回退冻结/降级演练/域级回归矩阵 + 首产段守恒总核与冻结总账；
  - 落盘 `docs/unxreal/batches/UNX-G2-B01..B15.md`（15 件骨架册，两行式条目，批头含防重声明/嫁接源/红线注记/批注）。
- **断言链（骨架态适用面）**：①防重四范围 grep——kernel 命中仅 wingl.rs/path3.rs/lib.rs/quality.rs llvmpipe 注释引用（E1 借力清单形态非实装，边界对账条 F25100 显式声明），其余范围零命中，300 条 ID+名唯一；②判据三成分 300 条齐（动作动词+可观测对象+可复测锚点）；③行数守恒——脚本实核 15 批逐批 6,000、求和 90,000 整零偏离、ID 连续 F24801–F25100；④台账回填（总纲 §7.3-G2 落盘+根台账/handoff）；⑤四项齐备（骨架态下限=判据句成型+行数预算登记）。
- **冻结接口预告**：winsys 联签三锚（B03，→G1/AI-31 会签）；GL/VK 能力面供给清单（B15，→F2 AI-27/G3 AI-33/G4 AI-34/E1 AI-21 消费）；CTS 账本导出 schema（B13，→O3 验收机器人判据库）。
- **open_risks**：R-G2-001（P1 G1 联签三锚未冻结，Schema 先行+fake，冻结后联签回归）/ R-G2-002（P2 真机判据 ≥150 条随闸门补测，双轨产线零 QEMU 零实机写）/ R-G2-003（P2 Mesa 季度换版风险级高，跟随账本+回退冻结机制就位）。
- **双同步**：docs 落盘 + git 提交推送。共享协调文件（根台账/总纲/handoff/汇编册）按统一台账 append-only 性质提交并登记背景；他会话产物均不纳入，不越权代提交。
"""
# append before ## 五、
SEC5 = "## 五、协作纪律速览（新 AI 会话必读）"
i = led.index(SEC5)
led = led[:i] + log.lstrip("\n") + "\n" + led[i:]
wr(LEDGER, led)
print("ledger ok")

# ---------- 4. handoff.json ----------
h = json.load(io.open(HANDOFF, encoding="utf-8"))
h["updated_at"] = "2026-10-02T12:00:00"
h["last_session"] = ("AI-32 G2 域认领扩表：§7.3-G2 40 批全表一次建入，B01–B15 认领写锁；首产段 300 项新功能/90,000 行"
 "（batches/UNX-G2-B01..B15.md，F24801–F25100 连续唯一，批批 6,000 守恒）；Gallium 嫁接地基与 winsys×G1 联签三锚（Schema 先行+fake R-G2-001）"
 "/iris/crocus+llvmpipe/GL state tracker/RADV/NVK/lavapipe/piglit/VK-GL-CTS 账本/季度跟随与降级演练/首产段收口十五主题；"
 "生成器 scripts/unxreal_g2_skeleton_gen.py、校验器 scripts/unxreal_g2_skeleton_check.py 五查 ALL PASS；"
 "汇编册部G 域节纯追加（合计 1178 册/23560 条/7,128,240 行）；根台账 §三/§四 同步（波13-M01）")
h["next_batch"] = "G2 续领 B16–B40 待令（余 150,000 行，主题框架图见总纲 §7.3-G2 小结）；他域待领以根台账为准"
h.setdefault("domain_ledger_progress", {})["G2"] = {
    "finalized_batches": 0,
    "skeleton_batches": 15,
    "skeleton_list": [f"UNX-G2-B{n:02d}" for n in range(1, 16)],
    "rows_locked": 90000,
    "rows_budget": 240000,
    "next_batch": "UNX-G2-B16（待领，B16–B40 余 150,000 行；闭账物 CTS 首轮全绿账波 13 AI-32 主办）",
    "open_risks": ["R-G2-001", "R-G2-002", "R-G2-003"],
}
h.setdefault("sync_notes", []).append("AI-32 G2 首产段四处同步（汇编册/总纲 §7.3-G2/根台账/handoff）——波13-M01")
json.dump(h, io.open(HANDOFF, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print("handoff ok")
