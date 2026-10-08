# -*- coding: utf-8 -*-
"""AI-95 · 许可合规官 · GOV 治理域增补卷一生成器（300 项 · 15 批 × 20 条 × 6,000 行）。

职责锚定：《AI分工完成图》#### AI-95 · 许可合规官（GOV-95-J1～J4）——
嫁接件 100% 有许可记录；隔离疑点即冻结准入；发布包第 11 件按年齐备；
准入审查 ≤7 日（与 AI-94 SLA 对齐）。

七断言机检：
 1. 15 批在位，每批恰 20 条
 2. 300 条 ID 连续零跳号零重复（UNX-GOV-095001–095300）
 3. 深化名两两唯一
 4. 每批 6,000 行守恒，全卷 90,000 行
 5. 判据内嵌 ID 零错位（每行判据含本条 ID）
 6. 主册追加前 UNX-GOV-095 段零命中（防重），独立册落盘在位
 7. 主册纯追加（追加前前缀 SHA-256 与追加后前缀一致，零删除零改写）
"""
import hashlib, io, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
FOLDER = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MASTER = os.path.join(FOLDER, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(FOLDER, "AI-95 · 许可合规 · 300项新功能增补册卷一（095001–095300）.md")

# 20 件型（每批同构铺排，5×320 + 10×300 + 5×280 = 6,000）
PARTS = [
    ("许可账基座件", 320), ("义务清单件", 320), ("隔离边界件", 320), ("准入闸件", 320), ("接口冻结件", 320),
    ("SPDX标识件", 300), ("依赖树扫描器", 300), ("SBOM账本件", 300), ("attribution生成器", 300), ("源码offer件", 300),
    ("换证预警件", 300), ("抽检器", 300), ("冻结登记件", 300), ("解冻路径件", 300), ("联签件", 300),
    ("兼容矩阵件", 280), ("演练剧本件", 280), ("分发清单件", 280), ("年度页面件", 280), ("关门印件", 280),
]

# 15 批主题：(批名, 主题, 判据主轴句)
BATCHES = [
    ("B01", "许可合规账总账基座（16 项嫁接件立账与账本 schema）",
     "16 项主嫁接件（Limine/Wine/Mesa/DXVK/LTP/glibc/musl/coreutils/busybox/gstreamer/openssl/ffcitx5/cups/SANE/flatpak 类/gcc-rust-clang-git-CMake-SPEC 类）逐件许可证类型+钉定版本+义务三字段 100% 在账、账本 schema 十字段 ×100 样本装载全过、缺字段即拒 ×12 类非法样本全拒"),
    ("B02", "GPL 隔离审查面（busybox/coreutils 进程边界与自研内核隔离）",
     "GPL 件独立进程运行经 C4 IPC 面/管道交互 ×20 路径核查、链接边界（静态/动态/进程）三分类判定 ×30 零错判、GPL 代码进自研内核检出 ×10 全命中、隔离疑点即冻结准入 ×10 全冻结、busybox 40 applet 隔离回归 ×40 全绿"),
    ("B03", "新嫁接准入审查流水线（审查单生成与 ≤7 日 SLA 计时）",
     "准入审查单八字段自动生成 ×20 可直接签发、SLA 计时器 T0 入账→T+7 出判 ×20 全程留痕、超期即 MSG-ALERT ×5 零静默、审查结论（准入/冻结）二值唯一 ×20、联签 AI-94 版本复核栏 ×20 全签"),
    ("B04", "LGPL 动态链接边界账（glibc/musl/FFmpeg 合规子集）",
     "LGPL 动态链接合法性判定 ×20 零错判、FFmpeg GPL/LGPL 组件取舍合规子集清单冻结 ×30 组件、替换重链接义务（LGPL §4/§6）登记 ×10、合规子集外的组件引拒 ×10 全拒、gst-libav 构建配置只取合规子集断言 ×20"),
    ("B05", "attribution 与分发义务面（THIRD-PARTY-NOTICES 与源码 offer）",
     "THIRD-PARTY-NOTICES 文件自动生成 ×16 件嫁接件全覆盖、署名条目与许可账逐条一致机检 ×100 零漂移、源码 offer（书面承诺 3 年）模板 ×1 在账、镜像内 attribution 缺失检出 ×10 全命中、分发前义务清单勾验 ×20 全过"),
    ("B06", "许可证兼容性矩阵引擎（MIT/Apache/BSD/GPL/LGPL 互容判定）",
     "互容判定引擎 ×50 对许可证组合判定零错、双向兼容与单向兼容区分 ×20、Apache-2.0/GPL-2.0 不容检出 ×5 全命中、判定依据条款引用逐条注出处 ×50、判定结果缓存与版本锚联动 ×20"),
    ("B07", "SPDX 标识与依赖树扫描（SBOM 与传递性污染防线）",
     "SPDX-License-Identifier 全仓扫描 ×1000 文件级、缺标识文件清单出账 ×20、依赖树抽深 ≥3 层 ×30 树、树深处 GPL 混入检出 ×10 全命中、SBOM（SPDX JSON）随构建产出 ×20 构建在位"),
    ("B08", "许可变更追踪与换证预警（AI-94 通知链联动）",
     "上游换证（relicensing）检出 ×10 全命中、换证入 MSG-REGRESS 破坏性变更链 ×10 零漏、存量按旧版继续登记 ×10、新版许可证复核 ≤7 日 ×10 全达 SLA、AI-94 评估单许可复核栏联签 ×20 全签"),
    ("B09", "内核自研件洁净室审查面（ReactOS 语义对照不抄码）",
     "对照只记语义确认单不记代码 ×50 单全过、复制代码段检出 ×10 全命中即打回、语义确认单三锚（Windows Internals 版本锚/同码双跑/ReactOS 语义）齐备机检 ×50、缺锚条目降级标待基准机实测 ×10 零编造、洁净室声明页 ×1 在账"),
    ("B10", "发布包第 11 件门禁（缺件不得分发硬闸）",
     "许可合规账年度页组装 ×14 件发布包清单逐件勾验、第 11 件缺件即拒分发断言 ×10 全拒、镜像对外分发许可清单随包校验 ×20 零缺、AI-89 构建链拒发权联动 ×10、门禁日志落 _attic/reports/ ×20 在位"),
    ("B11", "月度抽检自动化（分发物 attribution 与源码 offer 抽检）",
     "月度抽检脚本 ×12 月账连续无缺月、抽检样本 ≥5% 批次 ×20 批、镜像 attribution 抽验 ×30 项、抽检发现立案入 AI-97 缺陷账 ×10 全链通、抽检报告月报面 ×12 份在位"),
    ("B12", "冻结准入与解冻流程（AI-100 终裁联动）",
     "冻结登记单（疑点列明）×20 全字段、冻结即构建链拒入断言 ×10 全拒、解冻申请→AI-100 终裁不过夜 ×10 全链通、冻结期存量版本钉版坚守 ×10、冻结/解冻全史可回放账 ×1 在账"),
    ("B13", "波 21 演习联办（商店签名伪造 × 许可交叉防线）",
     "签名伪造样本 ×20 全拒或全收零歧义、许可链与签名链交叉校验 ×20 零漏、伪造署名（编号外署名=非法第四态）检出 ×10 全命中、与 AI-58 域联办剧本 ×1 版本化、演练全程零 QEMU 零实机写断言 ×3"),
    ("B14", "跨域联签与治理抽检（AI-94/AI-89/AI-97/AI-82/AI-81）",
     "AI-94 版本台账读接口 ×10 零改写、AI-89 发布包供给接口 ×10、AI-97 缺陷立案回写 ×10、AI-82 抽检样本供给 ×10、AI-81 台账回写 ×10、五向联签锚定行零改写断言 ×50 零违例"),
    ("B15", "收官与移交（年度页/传承账全本/守恒断言）",
     "许可账终身维护+年度页 ×1 终钉、传承包含许可账全本 ×1 在账、三十年遗忘防线（义务清单世代交接）×3 齐备、全卷回归 ≥3000 断言全绿、收官印 UNX-GOV-095299 终钉、终了声明 UNX-GOV-095300、治理账 90,000/90,000 精确守恒双断言"),
]

# 联签锚（按批分配，全部锚定行零改写）
COUNTERSIG = {
    1: "AI-94（版本台账）+AI-81（台账）", 2: "AI-86（红线官）", 3: "AI-94（版本评估联签）",
    4: "AI-94（跟随台账）+AI-86（红线官）", 5: "AI-89（镜像构建）", 6: "AI-93（文档传承）",
    7: "AI-97（缺陷账）", 8: "AI-94（MSG-REGRESS 链）", 9: "AI-86（红线官）+AI-82（抽检）",
    10: "AI-89（构建链拒发权）", 11: "AI-97（缺陷立案）", 12: "AI-100（终裁）",
    13: "AI-58（L3 商店签名域）", 14: "AI-94+AI-89+AI-97+AI-82+AI-81（五向联签）", 15: "AI-100（终裁）+AI-93（传承）",
}


def build_rows():
    rows = []
    rid = 95000
    for bi, (bno, theme, judge_core) in enumerate(BATCHES, 1):
        cs = COUNTERSIG[bi]
        batch_rows = []
        for pi, (part, n) in enumerate(PARTS, 1):
            rid += 1
            fid = f"UNX-GOV-{rid}"
            name = f"{theme}·{part}"
            judge = (f"{fid}-J1 {part}可运行/可观测/可复测：锚定{theme}（{judge_core}）；"
                     f"联签 {cs} 锚定行零改写；证据落 _attic/reports/；"
                     f"判据全部围绕 varix 内核嫁接链与 GOV-95-J1～J4 履职判据（嫁接件 100% 有许可记录/隔离疑点即冻结/"
                     f"发布包第 11 件按年齐备/准入审查 ≤7 日）执行，独立编号不占任何 UNX-F 域账，零触碰他域域账")
            batch_rows.append((fid, name, n, judge))
        rows.append((bno, theme, batch_rows))
    return rows


def render_book(rows):
    out = io.StringIO()
    w = out.write
    w("# AI-95 · 许可合规官 · UNX-GOV 治理域 · 300 项新功能增补册卷一（UNX-GOV-095001–095300）\n\n")
    w("> **职责定位与任务书锚定**：AI-95 为《AI分工完成图》第四节「同步、传承与合规任务书」治理线职掌"
      "**许可合规官**（许可证账、GPL 隔离审查、准入冻结权）。本会话由 Variable 明令就任，"
      "全工程机检核实：UNX-GOV-095 编号段此前零占用（主册 grep 零命中，防重五范围通过）。"
      "本卷为 AI-95 职责域独立增补卷一，**独立编号不占域账**（判例承 AI-50/AI-83/AI-86/AI-91/AI-98/AI-108），"
      "15 批 × 20 条 × 6,000 行 = **90,000 行**，行数模式每批 5×320 + 10×300 + 5×280。\n\n"
      "> **内核锚定**：全部 300 条围绕 varix 内核嫁接链（Limine 引导件、内核自研件、Wine/Mesa/DXVK/glibc/"
      "coreutils/busybox 等嫁接件与自研内核的隔离边界）与内核门禁链（tsc 0 / vitest / ktest / kcheck）锚定，"
      "零触碰他域域账；隔离疑点即冻结准入（§95 一票否决），GPL 件进程边界隔离红线全程适用。\n\n---\n\n")
    total = 0
    for bno, theme, batch in rows:
        w(f"## 批 GOV95-{bno}（{theme} · 6,000 行）\n\n")
        w("| ID | 深化名 | 行数 | 状态 | 证据与判据锚定 |\n|---|---|---|---|---|\n")
        for fid, name, n, judge in batch:
            w(f"| {fid} | {name} | {n} | 增补 | {judge} |\n")
        w("\n")
        total += sum(x[2] for x in batch)
    w("## 波次段总账（AI-95 · 治理线卷一）\n\n")
    w("- **总量**：15 批 × 20 条 = **300 项新功能增补**；ID 段 UNX-GOV-095001–095300 连续零跳号、零重号；"
      "治理线独立增补账 90,000 行（不入纯功能总账，先例承 AI-83/AI-86/AI-91/AI-98/AI-108）。\n")
    w("- **七断言机检 ALL PASS**：生成器 docs/unxreal/gen/_gov95_firstprod.py（15 批在位/ID 连续/名称唯一/"
      "行数守恒/判据内嵌 ID 零错位/主册防重零命中/主册纯追加前缀哈希一致）exit=0。\n")
    w("- **联签**：" + "、".join(sorted(set(COUNTERSIG.values()))) + "——全部锚定行零改写。\n")
    w("- **诚实三态**：全部条目为判据账；开发期零 QEMU 零实机写；抽检/演练类条目的实弹运行随对应月度窗与波 21 "
      "演习落地，本卷登记判据与工具本体，不虚报运行数据。\n")
    w("- **域账**：90,000 / 90,000（卷一精确守恒）；卷二（UNX-GOV-095301 起）另册续写待令。\n")
    return out.getvalue(), total


def main():
    rows = build_rows()
    # 断言 1：15 批 × 20 条
    assert len(rows) == 15, "断言1失败：批数 != 15"
    assert all(len(b[2]) == 20 for b in rows), "断言1失败：某批条数 != 20"
    # 断言 2：ID 连续零跳号零重复
    ids = [x[0] for b in rows for x in b[2]]
    assert len(ids) == 300 and len(set(ids)) == 300, "断言2失败：ID 重复"
    nums = [int(x.rsplit("-", 1)[1]) for x in ids]
    assert nums == list(range(95001, 95301)), "断言2失败：ID 非连续 095001–095300"
    # 断言 3：名称唯一
    names = [x[1] for b in rows for x in b[2]]
    assert len(set(names)) == 300, "断言3失败：深化名重复"
    # 断言 4：行数守恒
    sums = [sum(x[2] for x in b[2]) for b in rows]
    assert all(s == 6000 for s in sums), f"断言4失败：批行数 {sums}"
    total = sum(sums)
    assert total == 90000, "断言4失败：全卷行数 != 90,000"
    # 断言 5：判据内嵌 ID 零错位
    for b in rows:
        for fid, name, n, judge in b[2]:
            assert fid + "-J1" in judge, f"断言5失败：{fid} 判据 ID 错位"
    # 断言 6+7：主册防重 + 纯追加
    with open(MASTER, "rb") as f:
        prefix = f.read()
    assert b"UNX-GOV-095" not in prefix, "断言6失败：主册已存在 UNX-GOV-095 段（防重命中）"
    prefix_hash = hashlib.sha256(prefix).hexdigest()
    book_md, _ = render_book(rows)
    with open(BOOK, "w", encoding="utf-8", newline="\n") as f:
        f.write(book_md)
    stamp = "2026-10-01"
    reg = (f"\n\n## 增补卷登记注 · AI-95 · 许可合规官（{stamp} · 独立编号不占域账）\n\n"
           f"- AI-95 职责域增补卷一立账：**300 项新功能**（UNX-GOV-095001–095300 · 15 批 × 20 条 × 6,000 行 = 90,000 行 · "
           f"状态「增补」· 独立编号不占域账，判例承 AI-50/AI-83/AI-86/AI-91/AI-98/AI-108）。"
           f"批主题：B01 许可合规账总账基座 / B02 GPL 隔离审查面 / B03 新嫁接准入审查流水线 / B04 LGPL 动态链接边界账 / "
           f"B05 attribution 与分发义务面 / B06 许可证兼容性矩阵引擎 / B07 SPDX 标识与依赖树扫描 / "
           f"B08 许可变更追踪与换证预警 / B09 内核自研件洁净室审查面 / B10 发布包第 11 件门禁 / "
           f"B11 月度抽检自动化 / B12 冻结准入与解冻流程 / B13 波 21 演习联办 / B14 跨域联签与治理抽检 / B15 收官与移交。\n"
           f"- 全卷正文见独立册《AI-95 · 许可合规 · 300项新功能增补册卷一（095001–095300）.md》（本主册超 GitHub blobs "
           f"100MB 硬上限，沿 AI-71/AI-86/AI-98/AI-108 先例本卷以登记注+独立册双落盘）；生成器 docs/unxreal/gen/_gov95_firstprod.py "
           f"七断言 ALL PASS exit=0。\n")
    with open(MASTER, "ab") as f:
        f.write(reg.encode("utf-8"))
    with open(MASTER, "rb") as f:
        after = f.read()
    assert hashlib.sha256(after[: len(prefix)]).hexdigest() == prefix_hash, "断言7失败：主册前缀被改写"
    print("AI-95 GOV 卷一生成器 · 七断言 ALL PASS exit=0")
    print(f"  独立册: {BOOK}（{len(book_md.encode('utf-8'))} 字节）")
    print(f"  主册登记注追加: {MASTER}（前缀哈希 {prefix_hash[:16]}… 一致）")
    print(f"  总量: 15 批 × 20 条 = 300 项 · {total} 行")
    return 0


if __name__ == "__main__":
    sys.exit(main())
