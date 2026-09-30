# -*- coding: utf-8 -*-
"""AI-09 · handoff.json 增量编辑（基于磁盘现版，AI-07/AI-10 等全部记录保留）"""
import re, json, io

PATH = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\unxreal\handoff.json"
with io.open(PATH, 'r', encoding='utf-8', newline='') as f:
    t = f.read()
nl = '\r\n' if '\r\n' in t else '\n'
def J(s):  # 脚本内 \n → 文件实际行尾
    return s.replace('\n', nl)

# ---- 头部四字段 ----
new_updated = '"updated_at": "波08-M05（AI-09 B4 域 B01–B15 全批深化收口 300 条会话；前一版本为波04-M01 AI-10 B5 收口，其全部记录保留）"'
t2 = re.sub(r'"updated_at": "[^"]*"', new_updated.replace('\\', '\\\\'), t, count=1)
assert t2 != t, "updated_at not replaced"
t = t2

m = re.search(r'"last_session": "AI-10（[^"]*"', t)
assert m, "last_session anchor (AI-10) not found"
new_ls = '"last_session": "AI-09（CoRun 会话：B4 域 B01–B15 全批深化收口 300 条——B01–B08 块层栈 160 条 + B09–B15 NVMe 前段 140 条；finalize 五步断言链逐批全过：防重 300 条标题行唯一四范围零占用/判据三成分/行数守恒 90,000 零偏离（B15 F6683 元行缺失与 33 条短正文收口前修复归零留痕）/台账回填/四项齐备；深化正文 wc -m 合计 139,011 字；红线边缘 F6644/F6645 AI-86 双人复核登记；冻结接口 3（F6657 SMART/健康信息页→ATA/NVMe 归一化属性表）与 B5 联签；域累计 90,000/240,000（37.5%））"'
t = t[:m.start()] + new_ls + t[m.end():]

t2 = re.sub(r'"phase": "deepen（', '"phase": "deepen（B4 B01–B15 深化收口 300 条、冻结接口 3 登记、B16–B40 待领；', t, count=1)
assert t2 != t, "phase not prepended"
t = t2

t2 = re.sub(r'"next_batch": "', '"next_batch": "UNX-B4-B16 待领（AI-09 下会话按 §6.2 规则二续领，25 批 500 条预算余 150,000 行）；', t, count=1)
assert t2 != t, "next_batch not prepended"
t = t2

# ---- B4 块插入（progress 尾部，B5 块之后）----
b4_list = J("\n").join(f'        "UNX-B4-B{i:02d}",' for i in range(1, 16)).rstrip(",")
b4_block = J(f"""    }},
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
  "deepen_books": [""")
old_anchor = J('    }\n  },\n  "deepen_books": [')
assert t.count(old_anchor) == 1, f"B4 block anchor count={t.count(old_anchor)}"
t = t.replace(old_anchor, b4_block)

# ---- deepen_books 尾插 15 册 ----
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
books_json = J(",\n").join(f'    "{b}"' for b in books)
old_a = J('"\n  ],\n  "waves": {')
assert t.count(old_a) == 1, f"books anchor count={t.count(old_a)}"
t = t.replace(old_a, J('",') + "\n" + books_json + J('\n  ],\n  "waves": {'))

# ---- cross_domain_freeze 尾插（冻结接口 3）----
freeze_entry = J("""    },
    {
      "interface": "B4 冻结接口 3：SMART/健康信息页→ATA/NVMe 归一化属性表（F6657 · 12 列属性表）",
      "frozen_at": "波08-M05（AI-09 B4-B13 深化收口冻结，与 B5 联签中）",
      "consumers": [
        "B5",
        "B4 SMART 消费面（F6650/F6657）"
      ]
    }
  ],
  "open_risks": [""")
old_a = J('    }\n  ],\n  "open_risks": [')
assert t.count(old_a) == 1, f"freeze anchor count={t.count(old_a)}"
t = t.replace(old_a, freeze_entry)

# ---- open_risks 尾插三项 ----
risks_entry = J("""    },
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
  "adr_log": [""")
old_a = J('    }\n  ],\n  "adr_log": [')
assert t.count(old_a) == 1, f"risks anchor count={t.count(old_a)}"
t = t.replace(old_a, risks_entry)

# ---- sync_notes 尾插 ----
sync_entry = J("""不越权代提交。",
    "AI-09 波08-M05 提交范围仅限 AI-09 自有产物：deepen/B4-B01..B15.md（15 件七段式深化册 300 条）、总纲 §7.3-B4 十五行回填（[骨架]→[已深化]+实计字数 139,011）+B4 小结 finalize 记录、根统一协作总台账（§三/§四/§六 三处）、handoff.json 本文件（B4 块/deepen_books 15 册/cross_domain_freeze 冻结接口 3/open_risks 三项/sync_notes）。共享协调文件按统一台账 append-only 性质提交并登记背景；他会话产物（A1/A2/A5/B2/B3/B5/C2–C5/D2 等、docs/START 用户删除态文件）均不纳入，不越权代提交。"
  ]
}""")
old_a = J('不越权代提交。"\n  ]\n}')
assert t.count(old_a) == 1, f"sync anchor count={t.count(old_a)}"
t = t.replace(old_a, sync_entry)

with io.open(PATH, 'w', encoding='utf-8', newline='') as f:
    f.write(t)

# ---- 复核：JSON 合法性 + 结构 ----
d = json.loads(io.open(PATH, encoding='utf-8').read())
assert "B4" in d["domain_ledger_progress"]
assert d["domain_ledger_progress"]["B4"]["rows_locked"] == 90000
assert len([b for b in d["deepen_books"] if "B4-B" in b]) == 15
assert any(r["id"] == "R-B4-001" for r in d["open_risks"])
assert any("F6657" in fr["interface"] for fr in d["cross_domain_freeze"])
print("handoff.json edits OK; JSON valid; B4 block + 15 books + freeze + 3 risks registered")
print("progress keys:", list(d["domain_ledger_progress"].keys()))
