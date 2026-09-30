# -*- coding: utf-8 -*-
"""AI-09 · B4 域 finalize ④台账回填：总纲 §7.3-B4 + 根台账 + handoff.json"""
import re, sys

def load(path):
    with open(path, 'r', encoding='utf-8', newline='') as f:
        t = f.read()
    nl = '\r\n' if '\r\n' in t else '\n'
    return t, nl

def save(path, t):
    with open(path, 'w', encoding='utf-8', newline='') as f:
        f.write(t)

def edit(path, pairs, label):
    t, nl = load(path)
    for i, (old, new) in enumerate(pairs):
        o = old.replace('\n', nl)
        n = new.replace('\n', nl)
        c = t.count(o)
        assert c == 1, f"[{label}] pair#{i} anchor count={c}: {old[:70]!r}"
        t = t.replace(o, n)
    save(path, t)
    print(f"[{label}] {len(pairs)} edits OK")

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
OUTLINE = ROOT + r"\docs\Varix\CoRun Varix STAR II · Unxreal\CoRun Varix STAR II · Unxreal · 总纲与施工书.md"
LEDGER = ROOT + r"\CoRun Varix STAR II · Unxreal.md"
HANDOFF = ROOT + r"\docs\unxreal\handoff.json"

WC = {"B01":"9,781","B02":"8,331","B03":"7,616","B04":"7,252","B05":"7,464","B06":"7,682","B07":"6,692","B08":"6,971","B09":"7,576","B10":"11,283","B11":"11,878","B12":"11,953","B13":"12,533","B14":"11,008","B15":"10,991"}

# ============ 1) 总纲 §7.3-B4：15 行翻状态回填字数 ============
outline_pairs = []
for i in range(1, 16):
    b = f"B{i:02d}"
    lo, hi = 6400 + i*20 - 19, 6400 + i*20
    old = f"| UNX-B4-{b} | F{lo}–F{hi} | 20 | [骨架] | —（骨架行数待本批骨架册落定后回填实数） | AI-09 |"
    new = f"| UNX-B4-{b} | F{lo}–F{hi} | 20 | [已深化] | {WC[b]} | AI-09 |"
    outline_pairs.append((old, new))

# 总纲 B4 小结行
outline_pairs.append((
"**域 B4 台账小结**：40 批 · 800 条 · 域账 240,000 行。B01–B15 认领开工（AI-09 承接，Variable 明令本会话 300 条新深化：B01–B08 块层栈 160 条 + B09–B15 NVMe 前段 140 条，随 finalize 逐批回填三态与字数）· B16–B40 待领。上游 AI-02 MSI-X 分配接口（F0024 已由 AI-01 波 01 冻结交付）波 02 初对接；flush 契约波 03 中冻结交付 B2/B3/B5。",
"**域 B4 台账小结**：40 批 · 800 条 · 域账 240,000 行。B01–B15 深化收口（AI-09，Variable 明令本会话 300 条新深化：B01–B08 块层栈 160 条 + B09–B15 NVMe 前段 140 条，15 批 [已深化]，深化正文 wc -m 合计 139,011 字逐条实计 ≥300，深化行锁定 90,000/240,000 脚本实核零偏离）· B16–B40 待领（预算余 150,000 行）。上游 AI-02 MSI-X 分配接口（F0024 已由 AI-01 波 01 冻结交付）波 02 初对接；flush 契约波 03 中冻结交付 B2/B3/B5（F6610 契约对接已在 B05 三面收口）；冻结接口 3（F6657 SMART/健康信息页→ATA/NVMe 归一化属性表）与 B5 联签。"))

# 总纲 B4 修订记录追加
outline_pairs.append((
"> 修订记录（AI-09 会话）：按 §7.6 扩表规则 1 一次建入域 B4 全表 40 批；B01–B15 认领写锁（承接会话 AI-09）；扩表三项机械校验通过留痕于节首引言。",
"> 修订记录（AI-09 会话）：按 §7.6 扩表规则 1 一次建入域 B4 全表 40 批；B01–B15 认领写锁（承接会话 AI-09）；扩表三项机械校验通过留痕于节首引言。\n> 修订记录（AI-09 会话·finalize）：B01–B15 十五行 [骨架]→[已深化] 并回填深化字数（逐条 wc -m 实计：B01 9,781 / B02 8,331 / B03 7,616 / B04 7,252 / B05 7,464 / B06 7,682 / B07 6,692 / B08 6,971 / B09 7,576 / B10 11,283 / B11 11,878 / B12 11,953 / B13 12,533 / B14 11,008 / B15 10,991，合计 139,011）；行数守恒断言 15 批各 6,000、域累计 90,000 零偏离（脚本实核；B15 F6683 元行缺失与 B03–B09 共 33 条短正文收口前修复归零留痕）；防重四范围 grep——300 条标题行唯一、VE/CGPU/start/内核四范围零占用；四项齐备（300 条正文全部 ≥300 字六要素）；红线边缘条目 F6644/F6645（格式化类）AI-86 双人复核登记；冻结接口 3（F6657）与 B5 联签；验收判据 1 兑现批 B14（QD 五档曲线账 ≥10 断言/档 + 8 核扩展系数 ≥1.0 + 分流偏差 ≤20%）。"))

edit(OUTLINE, outline_pairs, "总纲§7.3-B4")

# ============ 2) 根台账：§三 B4 行 + §四 会话日志 + §六 修订行 ============
session_log = """### 会话 2026-波08-M05 · AI-09（B4 域 B01–B15 全批深化收口 300 条）
- **冷启动对账**：git + handoff.json + 总纲 §7.3-B4 三方对账；读入四份源册 AI-09 任务书全文（UNX-B4：块层与设备管理，F6401–F7200，40 批，240,000 行）与 §6 施工协议、§2.3 六要素模板、§7.6 扩表规则。B4 域认领扩表与 B01–B15 骨架 300 条为 AI-09 前序会话落盘（总纲 §7.3-B4 40 批全表在位），本会话承接深化收口；工作区他会话产物（A1/A2/A5/B2/B3/B5/D2 等在途与未跟踪件）保持原状不越权代管。
- **本会话 300 项新深化**（Variable 明令口径：一次对话 300 项；全部围绕 Varix 内核锚定：BlockDevice trait（blk.rs 290 行）、现存 nvme.rs 1,357 行轮询最小栈、ahci.rs 930 行、m700blk.rs 1,408 行按接管三字段口径升级）：
  - B01–B08 块层栈 160 条（F6401–F6560，行数锁定 48,000）：B01 bio/request 两级结构与 IO 提交面、B02 软件队列与 tag 分配（blk-mq 概念坐标）、B03 调度器 none（elevator 语义）、B04 mq-deadline（read_expire/write_expire/prio_aging）、B05 flush/FUA 三面（preflush/postflush/empty flush，F6610 契约对接 B2/B3/B5）、B06 gendisk 注册与设备命名/分区扫描、B07 统计判据账（fio/blktrace/WPT 观测坐标）、B08 块层收口治理（storport 验收语义/交接/回访）；
  - B09–B15 NVMe 前段 140 条（F6581–F6700，行数锁定 42,000）：B09 admin 扩展与识别面（CAP/identify）、B10 每核队列对与 doorbell（SqPair 八字段/P 位三型注入/cmdid 位图/PRP 链五档）、B11 命令集全量 I（DSM/NUSE/abort 三态机/ONCS 能力位总表/Unsupported 单点口径/fuzz ≥150 样本）、B12 错误处理与复位协议（五态状态机 ≥12 迁移边/CC.EN 四段复位序/三级升级链/只读降级/全位面 CQE 解码/错误注入库 ≥15 样本/每请求必有终态机检）、B13 多命名空间与特性集（双路扫描/NS 三段模型/格式化红线边缘 F6644–F6645/SMART 健康页 ≥12 组/冻结接口 3 F6657 归一化属性表/M5 联动预演/特性 fuzz ≥80 样本/域经 8 条入 DJ 库）、B14 QD 压测账（QD1/8/32/128/1024 五档曲线账 ≥10 断言/档+8 核扩展系数 ≥1.0+分流偏差 ≤20%+F6667 环境四件套机检+两轨分记禁混报）、B15 深化收口（接管完整性 100% 核对/现存 1,357 行回归哈希冻结/全链端到端 10^5 四层守恒/ktest ≥400 断言全绿/真机判据四组登记/三通道交接联签/1 小时 soak 三零/域级收官 37.5%）；
  - 落盘 `docs/unxreal/deepen/B4-B01..B15.md`（15 件七段式深化册，finalize 记账表逐册批尾落位）。
- **finalize 五步断言链（15 批逐批全过）**：①防重四范围 grep——300 条标题行（`^### UNX-F`）唯一、VE/CGPU/start/内核四范围零占用（跨条目引用按精确口径不计立账）；②判据三成分齐（真机/数值/对照，模拟器为测试宿主、真机判据登记随闸门补测）；③行数守恒——脚本实核 15 批逐批求和恰 6,000 行、域累计 90,000/240,000 零偏离（B15 F6683 元行缺失与 B03–B09 共 33 条短正文收口前修复归零留痕）；④台账回填——总纲 §7.3-B4 B01–B15 十五行 [骨架]→[已深化] 并回填实计字数（合计 139,011 字，逐批 ≥6,000 断言全过）；⑤四项齐备（300 条正文全部 ≥300 字六要素）。
- **治理件落位**：红线边缘条目 F6644/F6645（格式化类）AI-86 双人复核登记（写盘三重目标验证）；冻结接口 3（F6657 SMART/健康信息页→ATA/NVMe 归一化属性表）与 B5 联签；验收判据 1 兑现批 B14（QD 五档曲线账）；F6690 三通道交接联签与 F6682 现存栈回归哈希冻结。
- **open_risks**：R-B4-001（P1 格式化红线边缘 F6644/F6645 AI-86 双人复核前置）/ R-B4-002（P2 真机判据四组 F6685 随闸门补测，F6667 环境四件套+两轨分记禁混报机检）/ R-B4-003（P1 冻结接口 3 与 B5 联签、flush 契约 F6610 下游 B2/B3/B5 消费联签随上游收口）。
- **双同步**：docs 落盘 + git 提交推送（`unxreal(b4): B4 域 B01–B15 finalize 300条（AI-09 块层与设备管理 F6401–F6700 深化收口，行数守恒 90,000/240,000）`）。共享协调文件（根台账/总纲/handoff）按统一台账 append-only 性质提交并登记背景；他会话产物均不纳入，不越权代提交。

"""

ledger_pairs = [
# ① §三 B4 域行
("| UNX-B4 块层与设备管理 | AI-09 | 收口中（B01–B15） | B01–B15（15 批 300 条） | 0 | 回填随 finalize（预算 90,000 / 240,000） | AI-09 认领扩表（§7.3-B4 全表 40 批一次建入）；B01–B08 块层栈 + B09–B15 NVMe 前段；现存 nvme.rs/ahci.rs/blk.rs/m700blk.rs 按接管三字段口径深化 |",
"| UNX-B4 块层与设备管理 | AI-09 | B01–B15（15 批收口） | B01–B15（15 批 300 条） | 0 | 90,000 / 240,000（深化行数逐批求和，脚本实核零偏离；深化正文合计 139,011 字） | 波 08 窗；B4 域 §7.3 全表 40 批一次扩表 + B01–B15 全批深化收口 300 条（finalize 断言链逐批全过、行数守恒脚本实核零偏离）：B01–B08 块层栈（bio/request 两级/合并器/调度器 none 与 mq-deadline/flush FUA 三面/gendisk 注册/统计判据账/收口治理）+ B09–B15 NVMe 前段（admin 扩展/每核队列对与 doorbell/命令集全量/错误复位五态机/多命名空间与特性集/QD 五档压测账/收口总验证）；现存 nvme.rs 1,357 行/ahci.rs 930 行/blk.rs 290 行/m700blk.rs 1,408 行按接管三字段口径深化；红线边缘 F6644/F6645 双人复核登记；冻结接口 3（F6657）与 B5 联签；B16–B40 待领 |"),
# ② §四 会话日志追加（锚 §五 标题前插）
("## 五、协作纪律速览（新 AI 会话必读）", session_log + "## 五、协作纪律速览（新 AI 会话必读）"),
# ③ §六 修订行追加（锚 C2 修订行尾）
("域收官断言 F9100 三断言留痕、号面冻结体系五件套（F9093/F9094/F9096/F9099/F9100）闭合 |",
"域收官断言 F9100 三断言留痕、号面冻结体系五件套（F9093/F9094/F9096/F9099/F9100）闭合 |\n| 波08-M05 | AI-09 | B4 域 B01–B15 全批深化收口 300 条/90,000 行（总纲 §7.3-B4 十五回填 [已深化]+实计字数 139,011；handoff 增 B4 块/deepen_books 15 册/冻结接口 3/三项风险账；§三/§四/§六 同步）；finalize 断言链逐批全过、行数守恒脚本实核零偏离（B15 F6683 元行缺失与 33 条短正文收口前修复归零留痕）；红线边缘 F6644/F6645 双人复核登记、冻结接口 3（F6657）与 B5 联签、验收判据 1 兑现批 B14 |"),
]
edit(LEDGER, ledger_pairs, "根台账")

# ============ 3) handoff.json：头部四字段 + B4 块 + deepen_books + freeze + risks + sync_notes ============
b4_list = "\n".join(f'        "UNX-B4-B{i:02d}",' for i in range(1, 16)).rstrip(",")
b4_block = f"""    }},
    "B4": {{
      "finalized_batches": 15,
      "finalized_list": [
{b4_list}
      ],
      "skeleton_batches": 0,
      "rows_locked": 90000,
      "rows_budget": 240000,
      "rows_deepened_locked": 90000,
      "entries_skeleton": 0,
      "entries_deepened": 300,
      "entries_deepened_note": "B01–B08 块层栈 160 条（F6401–F6560，48,000 行）：B01 bio/request 两级与 IO 提交面 + B02 软件队列与 tag 分配 + B03 调度器 none + B04 mq-deadline + B05 flush/FUA 三面（F6610 契约对接 B2/B3/B5）+ B06 gendisk 注册与分区扫描 + B07 统计判据账 + B08 块层收口治理；B09–B15 NVMe 前段 140 条（F6581–F6700，42,000 行）：B09 admin 扩展与识别面 + B10 每核队列对与 doorbell/PRP 链五档 + B11 命令集全量（ONCS 能力位总表/Unsupported 单点/fuzz ≥150）+ B12 错误复位（五态机 ≥12 迁移边/CC.EN 四段序/三级升级链/错误注入 ≥15 样本/每请求必有终态机检）+ B13 多命名空间与特性集（红线边缘 F6644–F6645/SMART ≥12 组/冻结接口 3 F6657/特性 fuzz ≥80）+ B14 QD 压测账（五档 ≥10 断言/档+8 核扩展 ≥1.0+分流偏差 ≤20%+F6667 环境四件套机检）+ B15 收口总验证（接管 100% 核对/现存 1,357 行回归哈希冻结/全链 10^5 四层守恒/ktest ≥400 断言/三通道交接联签/1h soak 三零）；finalize 五步断言链逐批全过；深化正文 wc -m 合计 139,011 字；B16–B40 待领（预算余 150,000 行）",
      "open_bugs": 0
    }}
  }},
  "deepen_books": ["""

books = [
 "docs/unxreal/deepen/B4-B01.md（20 条新深化，F6401–F6420 bio/request 两级与 IO 提交面，正文 9,781 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B02.md（20 条新深化，F6421–F6440 软件队列与 tag 分配，正文 8,331 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B03.md（20 条新深化，F6441–F6460 调度器 none，正文 7,616 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B04.md（20 条新深化，F6461–F6480 mq-deadline 调度器，正文 7,252 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B05.md（20 条新深化，F6481–F6500 flush/FUA 三面与 F6610 契约对接，正文 7,464 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B06.md（20 条新深化，F6501–F6520 gendisk 注册与设备命名/分区扫描，正文 7,682 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B07.md（20 条新深化，F6521–F6540 块层统计判据账与观测面，正文 6,692 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B08.md（20 条新深化，F6541–F6560 块层收口治理与验收交接，正文 6,971 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B09.md（20 条新深化，F6561–F6580 NVMe admin 扩展与识别面，正文 7,576 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B10.md（20 条新深化，F6581–F6600 每核队列对与 doorbell/PRP 链五档，正文 11,283 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B11.md（20 条新深化，F6601–F6620 NVMe 命令集全量 I（能力位总表/Unsupported 单点/fuzz ≥150），正文 11,878 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B12.md（20 条新深化，F6621–F6640 NVMe 错误处理与复位协议（五态机/错误注入库/终态机检），正文 11,953 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B13.md（20 条新深化，F6641–F6660 NVMe 多命名空间与特性集（红线边缘复核/冻结接口 3），正文 12,533 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B14.md（20 条新深化，F6661–F6680 NVMe QD 压测账与扩展性（验收判据 1 兑现批），正文 11,008 字，6,000 行锁定零偏离）",
 "docs/unxreal/deepen/B4-B15.md（20 条新深化，F6681–F6700 NVMe 深化收口与接管总验证（域收官批），正文 10,991 字，6,000 行锁定零偏离）",
]
books_json = ",\n".join(f'    "{b}"' for b in books)
books_tail = ('"\n  ],\n  "waves": {', '",' + "\n" + books_json + '\n  ],\n  "waves": {')

freeze_entry = """    },
    {
      "interface": "B4 冻结接口 3：SMART/健康信息页→ATA/NVMe 归一化属性表（F6657 · 12 列属性表）",
      "frozen_at": "波08-M05（AI-09 B4-B13 深化收口冻结，与 B5 联签中）",
      "consumers": [
        "B5",
        "B4 SMART 消费面（F6650/F6657）"
      ]
    }
  ],
  "open_risks": ["""

risks_entry = """    },
    {
      "id": "R-B4-001",
      "text": "红线边缘条目 F6644/F6645（格式化类，涉写盘三重目标验证）动工前须 AI-86 双人复核（引导设施红线族延伸）",
      "owner": "AI-86",
      "level": "P1"
    },
    {
      "id": "R-B4-002",
      "text": "真机判据四组（F6685）登记随闸门补测（双轨产线：NVMe 模拟器为测试宿主，开发期零 QEMU/零实机写）；F6667 环境声明四件套（设备模型/核数/负载/时长）+ 两轨分记禁混报机检",
      "owner": "AI-09",
      "level": "P2"
    },
    {
      "id": "R-B4-003",
      "text": "冻结接口 3（F6657 SMART/健康信息页→ATA/NVMe 归一化属性表）与 B5 域联签待收口；flush 契约（F6610）下游 B2/B3/B5 消费联签随上游 finalize；现存栈接管（nvme.rs 1,357 行/ahci.rs 930 行/blk.rs 290 行/m700blk.rs 1,408 行）按接管三字段口径深化，AI-84 复核非重复口径",
      "owner": "AI-09",
      "level": "P1"
    }
  ],
  "adr_log": ["""

sync_entry = """不越权代提交。",
    "AI-09 波08-M05 提交范围仅限 AI-09 自有产物：deepen/B4-B01..B15.md（15 件七段式深化册 300 条）、总纲 §7.3-B4 十五行回填（[骨架]→[已深化]+实计字数 139,011）+B4 小结 finalize 记录、根统一协作总台账（§三/§四/§六 三处）、handoff.json 本文件（B4 块/deepen_books 15 册/cross_domain_freeze 冻结接口 3/open_risks 三项/sync_notes）。共享协调文件按统一台账 append-only 性质提交并登记背景；他会话产物（A1/A2/A5/B2/B3/B5/C2–C5/D2 等、docs/START 用户删除态文件）均不纳入，不越权代提交。"
  ]
}"""

handoff_pairs = [
    (r'"updated_at": "波07-M02（AI-17 D2 域 B01–B15 深化 300 条收口会话；前一版本为波08-M04 AI-12 C2 收口，其全部记录保留）",',
     '"updated_at": "波08-M05（AI-09 B4 域 B01–B15 全批深化收口 300 条会话；前一版本为波07-M02 AI-17 D2 收口，其全部记录保留）",'),
    (r'"phase": "deepen（D2 B01–B15 深化收口 300 条、F12966/F13098 跨域冻结、B16–B40 待领；A1 B01–B17 深化收口+B18 起步 3/20；C2/C3/C5/B3 全域收口；B2 B01–B02 深化；多域并行不冲突）",',
     '"phase": "deepen（B4 B01–B15 深化收口 300 条、冻结接口 3 登记、B16–B40 待领；A1 B18 起步 3/20 待续 F0344；C2/C3/C5/B3/B5/D2 全域收口；B2 B01–B02 深化；多域并行不冲突）",'),
    (r'"next_batch": "UNX-A1-B18 续 F0344 起 17 条（AI-01 下会话续接收口后转 B19）；C2 B16–B40 待领（AI-12 下会话续领，F9101 起）；UNX-D2-B16 起待领（AI-17 后续会话续领，25 批 500 条）；其他域并行指针不变",',
     '"next_batch": "UNX-B4-B16 起待领（AI-09 下会话续领，F6701 起 25 批 500 条）；UNX-A1-B18 续 F0344 起 17 条（AI-01 下会话续接收口后转 B19）；C2 B16–B40 待领（AI-12 下会话续领，F9101 起）；UNX-D2-B16 起待领（AI-17 后续会话续领，25 批 500 条）；其他域并行指针不变",'),
    ('    }\n  },\n  "deepen_books": [', b4_block),
    books_tail[0], books_tail[1],
    ('        "B4"\n      ]\n    }\n  ],\n  "open_risks": [', freeze_entry),
    ('      "level": "P2"\n    }\n  ],\n  "adr_log": [', risks_entry),
    ('不越权代提交。"\n  ]\n}', sync_entry),
]
# last_session 整行替换（regex）
t, nl = load(HANDOFF)
m = re.search(r'"last_session": "AI-17（[^\n]*",', t)
assert m, "last_session anchor not found"
new_ls = '"last_session": "AI-09（CoRun 会话：B4 域 B01–B15 全批深化收口 300 条——B01–B08 块层栈 160 条 + B09–B15 NVMe 前段 140 条；finalize 五步断言链逐批全过：防重 300 条标题行唯一四范围零占用/判据三成分/行数守恒 90,000 零偏离（B15 F6683 元行缺失与 33 条短正文收口前修复归零留痕）/台账回填/四项齐备；深化正文 wc -m 合计 139,011 字；红线边缘 F6644/F6645 AI-86 双人复核登记；冻结接口 3（F6657 SMART/健康信息页→ATA/NVMe 归一化属性表）与 B5 联签；域累计 90,000/240,000（37.5%））",'
t = t[:m.start()] + new_ls + t[m.end():]
save(HANDOFF, t)
edit(HANDOFF, handoff_pairs, "handoff.json")

# ============ 4) 复核 ============
t, _ = load(HANDOFF)
import json
json.loads(t)
print("[复核] handoff.json 合法 JSON ✓")
for p, key in [(OUTLINE, "[已深化] | 9,781"), (LEDGER, "90,000 / 240,000（深化行数逐批求和")]:
    t, _ = load(p)
    print(f"[复核] {p.split(chr(92))[-1]}: 含锚 {key!r} ×{t.count(key)}")
print("ALL DONE")
