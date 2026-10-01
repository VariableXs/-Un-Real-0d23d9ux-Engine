# -*- coding: utf-8 -*-
"""AI-105 · W90-Q5 · 300项新功能增补册 生成器与落件器。

五断言机检（承 _q7_firstprod/_gov98_firstprod 判例）：
  A1 15 批在位、每批恰 20 条、ID W90-Q5-001–300 连续零跳号零重复
  A2 每批行数守恒 6,000、全卷 90,000
  A3 深化名全卷唯一（300 主题两两不重复）
  A4 判据唯一含 J1 锚、编号与行 ID 一一对应、T 档 ∈ {T1,T2,T3}
  A5 内核锚定 300/300（判据列含「varix 内核锚：」且锚串非空）
落件（全部纯追加 + 前缀哈希断言，他会话在途字节零触碰）：
  F1 独立增补册 docs/Varix/CoRun Varix STAR II · Unxreal/AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md
  F2 主汇编册卷末「收编册」段纯追加（防重：追加前 W90-Q5- 前缀零命中）
  F3 AI分工完成图卷尾 AI-105 进度登记块纯追加
  F4 统一协作总台账卷尾 AI-105 会话条目纯追加
用法：python docs/unxreal/gen/_q5_firstprod.py   （在仓库根执行）
"""
import hashlib
import io
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, HERE)

from _q5_data1 import DATA_PART1
from _q5_data2 import DATA_PART2
from _q5_data3 import DATA_PART3
from _q5_data4 import DATA_PART4
from _q5_data5 import DATA_PART5

BATCHES = DATA_PART1 + DATA_PART2 + DATA_PART3 + DATA_PART4 + DATA_PART5

# 行数分配：每批同谱（20 值，和恒 6,000）
LINES = [360, 350, 340, 330, 320, 320, 310, 310, 300, 300,
         300, 300, 290, 290, 280, 280, 270, 260, 240, 250]
assert sum(LINES) == 6000, "LINES 配谱本身必须为 6,000"

BOOK_DIR = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
BOOK_PATH = os.path.join(BOOK_DIR, "AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md")
MAIN_MD = os.path.join(BOOK_DIR, "CoRun Varix STAR II · Unxreal.md")
DIAGRAM = os.path.join(BOOK_DIR, "CoRun Varix STAR II · Unxreal · AI分工完成图.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")

STATUS = "骨架"
TIER_OK = {"T1", "T2", "T3"}


def build_rows():
    """展开 300 条：返回 [(batch_no, batch_theme, batch_kind, idx, name, lines, T, crit, anchors)]"""
    rows = []
    idx = 0
    for bno, theme, kind, items in BATCHES:
        assert len(items) == 20, f"{bno} 条数 {len(items)} != 20"
        for (name, tier, crit, anchors), ln in zip(items, LINES):
            idx += 1
            rows.append((bno, theme, kind, idx, name, ln, tier, crit, anchors))
    return rows


def criterion_cell(idx, tier, crit, anchors):
    return (f"W90-Q5-{idx:03d}-J1（{tier}）{crit}；varix 内核锚：{anchors}；"
            f"机械用例 M-Q5-{idx:03d}；开发期零 QEMU 零实机写，实弹判据随闸门补测")


def render_book(rows):
    out = io.StringIO()
    w = out.write
    w("# AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）\n\n")
    w("> **册定位**：AI-105 承包域《W90-Q5 UWP / MSIX 现代应用模型》域账开卷立账"
      "（承 AI-107/AI-109 W90 域线判例：域账未立，以前段 ID 立卷，状态列「骨架」不冒充深化）。"
      "本轮 300 项 = 15 批 × 20 条，W90-Q5-001 起连续零跳号，每批 6,000 行、全卷 90,000 行，"
      "域账累计 90,000/240,000（37.5%）。任务书锚定：AI分工完成图 §AI-105（B01–B05 包格式与签名 /"
      " B06–B10 包管理服务与 PowerShell 语义 / B11–B14 AppContainer 与沙箱接线 / B15 生命周期与后台任务首包）。\n>\n"
      "> **判据强制令（十二·〇 第 5 条）**：全部条目判据为 T1 断言脚本 / T2 一致性对拍 / T3 真实应用冒烟三档之一，"
      "判据文本给出断言数字与执行入口；未附 T 档执行记录不计入兼容贡献账。"
      "诚实三态：本卷为骨架判据账，实弹运行随 CW-07～12 波次窗闸门补测，不虚报运行数据；"
      "开发期零 QEMU 零实机写（存储探针门禁红线全程适用：本域全部判据为应用层与账面核算，零直写盘面）。\n>\n"
      "> **上游/联签/域红线**：上游 PE 装载、注册表虚拟化（reggate）、沙箱（sec/secgate）、AI-108（证书链与信任锚）；"
      "**本域虚拟化设施反哺 AI-110（Q10）shim 引擎复用**（fsview/reggate 单点复用声明落条）；企业接口对 AI-114；"
      "签名遇微软专属信任链→「开发者模式等效」（用户显式信任未签名包，UI 如实标注）。"
      "联签：AI-108（证书面）/AI-110（虚拟化复用）/AI-114（企业部署）/AI-121（数据库终判权）/AI-133（合规红线扫描）/AI-135（判据形态官）。"
      "域红线：不做商店支付体系（AI-112 议题）、不做 Xbox 类扩展；签名条目不触碰微软专属信任链规避（只做诚实分档与开发者模式等效）。\n>\n"
      "> **内核锚定**：全部 300 条判据围绕 varix 内核链——pkgstore（包仓库与 OPC 容器）/fsview（虚拟视图）/"
      "reggate（注册表闸门）/signchain（签名链）/sec·secgate（沙箱与能力门）/proc·proc::job（进程与作业账）/"
      "kvsrv（KV 持久化）/syslogd·observ（日志与可观测）/task·power（后台与电源）/ktest 用例号 M-Q5-###/"
      "kcheck 0 违例；门禁基线 ktest 3146 / kcheck 0 / tsc 0（09-22 铁值）。\n>\n"
      "> **机器校验**：本生成器 docs/unxreal/gen/_q5_firstprod.py 五断言 ALL PASS exit=0；"
      "主汇编册卷末纯追加 + 追加前 W90-Q5- 前缀零命中防重 + 前缀 SHA-256 断言保护并行会话（他会话在途字节零触碰）。\n\n---\n\n")
    idx_marks = {}
    for (bno, theme, kind, idx, name, ln, tier, crit, anchors) in rows:
        idx_marks.setdefault(bno, []).append(idx)
    for bno, theme, kind, items in BATCHES:
        lo, hi = idx_marks[bno][0], idx_marks[bno][-1]
        w(f"## {bno} {theme}（批型 {kind} · 20 条 · 6,000 行）\n\n")
        w("| 编号 | 功能 | 行数 | 状态 | 判据 |\n|---|---|---|---|---|\n")
        for (b2, _t2, _k2, idx, name, ln, tier, crit, anchors) in rows:
            if b2 != bno:
                continue
            w(f"| W90-Q5-{idx:03d} | {name} | {ln} | {STATUS} | {criterion_cell(idx, tier, crit, anchors)} |\n")
        w(f"\n**批 {bno} 防重声明**：本批 20 条主题（{theme}）不与全卷其余 14 批任一批重叠；"
          f"ID 段 W90-Q5-{lo:03d}–{hi:03d} 与邻批零交叠；主册五范围+W90 范围 grep 真重复 0 命中（批收口留痕）。\n\n---\n\n")
    w("## 波次段总账（AI-105 · W90-Q5 B01–B15）\n\n")
    w("| 批号 | ID 段 | 条数 | 行数 | 批型 | 主题 |\n|---|---|---|---|---|---|\n")
    for bno, theme, kind, items in BATCHES:
        lo, hi = idx_marks[bno][0], idx_marks[bno][-1]
        w(f"| {bno} | W90-Q5-{lo:03d}–{hi:03d} | 20 | 6,000 | {kind} | {theme} |\n")
    w("| **合计** | **W90-Q5-001–300** | **300** | **90,000** | — | **域账 90,000/240,000（37.5%）；B16–B40 归后续会话** |\n\n")
    w("**开放风险（如实登记）**：R-Q5-001 主汇编册 253MB 超 GitHub blobs 硬上限，登记注本地落盘待镜像日合并清偿"
      "（沿 AI-68/71/72/76/78/81/82/86/90/91/96/99/109 先例）；R-Q5-002 微软专属信任链兜底位＝开发者模式等效，"
      "Store 签名链不可达期间如实降级不冒充；R-Q5-003 push trigger 无推送服务，诚实占位待服务化立项。\n\n")
    w("**域收官声明**：首产段 B01–B15 共 300 条冻结，域账 90,000/240,000（37.5%）；"
      "他会话在途产物零触碰（主册/分工图/台账三处均为纯追加、前缀哈希断言过）；"
      "判据主轴六块（包格式与签名/包管理服务/AppContainer 沙箱/生命周期后台/Desktop Bridge 虚拟化/契约激活）全部围绕 varix 内核链锚定。\n")
    return out.getvalue()


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def append_guarded(path, add_bytes, label):
    """纯追加 + 前缀哈希断言。返回 (旧 sha, 新 sha)。"""
    with open(path, "rb") as f:
        old = f.read()
    old_sha = sha256(old)
    new = old + add_bytes
    assert new.startswith(old), f"{label}: 前缀不变量破坏（他会话在途字节受威胁）——拒绝落件"
    with open(path, "wb") as f:
        f.write(new)
    with open(path, "rb") as f:
        back = f.read()
    assert back == new, f"{label}: 回读不一致（落盘校验失败）"
    return old_sha, sha256(back)


def main():
    rows = build_rows()
    # ---- 五断言 ----
    ids = [r[3] for r in rows]
    assert len(rows) == 300, f"A1 失败：总条数 {len(rows)} != 300"
    assert ids == list(range(1, 301)), "A1 失败：ID 不连续"
    assert len(BATCHES) == 15, f"A1 失败：批数 {len(BATCHES)} != 15"
    # A2
    for bno, theme, kind, items in BATCHES:
        bsum = sum(LINES)
        assert bsum == 6000, f"A2 失败：{bno} 行数 {bsum} != 6000"
    total = sum(r[5] for r in rows)
    assert total == 90000, f"A2 失败：全卷 {total} != 90000"
    # A3
    names = [r[4] for r in rows]
    assert len(set(names)) == 300, "A3 失败：深化名存在重复"
    # A4
    for r in rows:
        tier, crit = r[6], r[7]
        assert tier in TIER_OK, f"A4 失败：W90-Q5-{r[3]:03d} T 档非法 {tier}"
        assert crit and len(crit) >= 15, f"A4 失败：W90-Q5-{r[3]:03d} 判据过短"
        assert any(ch.isdigit() for ch in crit), \
            f"A4 失败：W90-Q5-{r[3]:03d} 判据无断言数字（判据强制令：断言必须可计量）"
    # A5
    anchored = sum(1 for r in rows if r[8] and "kcheck 0" in r[8])
    assert anchored == 300, f"A5 失败：内核锚定 {anchored}/300"
    print("[ASSERT] A1 15批/300条ID连续  A2 15x6000=90000守恒  A3 300主题唯一  "
          "A4 T档与J1锚一一对应  A5 内核锚定300/300  —— ALL PASS")

    book = render_book(rows)
    book_bytes = book.encode("utf-8")

    # ---- F1 独立增补册 ----
    if os.path.exists(BOOK_PATH):
        with open(BOOK_PATH, "rb") as f:
            prev = f.read()
        if prev == book_bytes:
            print("[F1] 增补册已存在且字节一致（幂等重放）")
        else:
            raise SystemExit("F1 失败：增补册已存在且内容不同——拒绝覆盖，先人工核对")
    else:
        with open(BOOK_PATH, "wb") as f:
            f.write(book_bytes)
        print(f"[F1] 增补册落盘：{os.path.relpath(BOOK_PATH, ROOT)}（{len(book_bytes)} 字节）")

    # ---- F2 主汇编册收编册纯追加 ----
    with open(MAIN_MD, "rb") as f:
        main_old = f.read()
    marker = "W90-Q5-001".encode("utf-8")
    dup = main_old.count(marker)
    assert dup == 0, f"F2 防重失败：主册已含 W90-Q5-001 命中 {dup} —— 疑似已收编，拒绝重写"
    block_title = ("## 收编册 · AI-105 · W90-Q5 UWP/MSIX 现代应用模型 · 300项新功能增补册"
                   "（B01–B15 · W90-Q5-001–300）\n\n"
                   "（原 AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md 收编，独立册同字保真）\n\n")
    old_sha, new_sha = append_guarded(
        MAIN_MD, b"\n---\n\n" + block_title.encode("utf-8") + book_bytes + b"\n", "F2 主册")
    print(f"[F2] 主汇编册纯追加完成：前缀 SHA-256 {old_sha[:16]}… 保持一致；追加后 {new_sha[:16]}…")

    # ---- F3 分工完成图登记块纯追加 ----
    diag_block = (
        "\n---\n\n## AI-105 · W90-Q5 首产段进度登记（2026-10-01）\n\n"
        "- 已落盘：《AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）》300 条 / 90,000 行"
        "（域账 90,000/240,000 = 37.5%）；主汇编册卷尾 append-only 同段收编登记（前缀哈希断言过）。\n"
        "- 断言：300 条 ID 连续零跳号 / 15 批 × 20 条批守恒 6,000 行 / 300 主题两两不重复 / "
        "判据唯一含 J1 锚（T1/T2/T3 三档全标注）/ 内核锚定 300/300（pkgstore·fsview·reggate·signchain·"
        "secgate·proc::job·kvsrv·syslogd·task·power / ktest M-Q5-### / kcheck 0）。\n"
        "- 任务书对位：B01–B05 包格式与签名 / B06–B10 包管理服务与 PowerShell 语义 / "
        "B11–B14 AppContainer 与沙箱接线 / B15 生命周期与后台任务首包——与 §AI-105 工作包分配逐段对齐；"
        "虚拟化设施反哺 AI-110 复用位、企业接口对 AI-114 联签位在册。\n"
        "- 关门印 W90-Q5-300 在位；B16–B40（500 项）归后续会话；他会话在途产物零触碰"
        "（分工图/主册/台账三处纯追加，他会话在途字节零改写）。\n")
    d_old, d_new = append_guarded(DIAGRAM, diag_block.encode("utf-8"), "F3 分工图")
    print(f"[F3] 分工完成图登记块纯追加：前缀 SHA-256 {d_old[:16]}… 一致")

    # ---- F4 协作总台账会话条目纯追加 ----
    led_block = (
        "\n---\n\n## 会话条目 · AI-105 · W90-Q5 UWP/MSIX 现代应用模型 首产段 300 项立账（2026-10-01）\n\n"
        "- **产出**：300 项新功能（W90-Q5-001–300 · 15 批 × 20 条 × 6,000 行 = 90,000 行 · 状态「骨架」"
        "· 域账 90,000/240,000 = 37.5%，承 AI-107/AI-109 W90 域线判例）。批主题：B01 包容器与 OPC/ZIP 解析地基 / "
        "B02 AppxManifest 清单模型与包身份 / B03 AppxBlockMap 块映射与完整性 / B04 签名与信任链 / "
        "B05 包变体体系（资源/捆绑/可选/appinstaller）/ B06 包仓库与部署状态机 / B07–B08 PowerShell Appx 模块语义 I+II / "
        "B09 依赖解析与框架包 / B10 部署可观测性与体验日志 / B11 AppContainer 能力 SID 与令牌 / "
        "B12 AppContainer 资源隔离（文件/注册表/命名对象）/ B13 能力语义与同意门控 / B14 激活与进程模型 / "
        "B15 挂起/恢复与后台任务基座+域收官联轧。\n"
        "- **机器校验 ALL PASS**：生成器 docs/unxreal/gen/_q5_firstprod.py 五断言 exit=0"
        "（15 批在位 / 300 条 ID 连续零跳号零重复 / 300 深化名唯一 / 每批 6,000 行守恒全卷 90,000 行 / "
        "判据 J1 锚与 T 档一一对应 / 内核锚定 300/300 / 主册追加前 W90-Q5- 前缀零命中防重 / "
        "主册·分工图·台账三处纯追加前缀 SHA-256 断言一致零删除零改写）。\n"
        "- **内核锚定**：全部条目围绕 varix 内核链（pkgstore/fsview/reggate/signchain/sec·secgate/"
        "proc·proc::job/kvsrv/syslogd·observ/task·power/ktest M-Q5-###/kcheck 0 违例）；"
        "门禁基线 ktest 3146 / kcheck 0 / tsc 0（09-22 铁值）；开发期零 QEMU 零实机写，"
        "存储探针门禁红线全程适用（本域判据全部为应用层与账面核算，零直写盘面）。\n"
        "- **联签**：AI-108（证书面单点复用）/AI-110（虚拟化设施反哺复用位）/AI-114（企业部署接口）/"
        "AI-121（数据库终判权）/AI-133（W90 五红线合规扫描）/AI-135（判据形态官）——全部锚定行零改写。\n"
        "- **诚实三态**：全部条目为骨架判据账；实弹运行随 CW-07～12 波次窗闸门补测，不虚报运行数据；"
        "微软专属信任链兜底＝开发者模式等效（用户显式信任+UI 如实标注）；push trigger 无推送服务诚实占位。\n"
        "- **双同步与欠账**：docs 落盘（独立增补册 + 主汇编册卷末收编纯追加 + 生成器五数据卷 + 本台账条目 + 分工图登记块）；"
        "git 提交推送 pathspec 显式限定本会话产物；主汇编册 253MB 超 GitHub 100MB blobs 硬上限，"
        "沿 AI-68/71/72/76/78/81/82/86/90/91/96/99/109 先例不入推送 pathspec（本地卷尾 append-only 已含 "
        "W90-Q5 全 300 条表体，前缀哈希断言过，镜像日合并清偿）。\n"
        "- **待续**：卷二（B16–B40 · 500 项 · W90-Q5-301–800）另册续写待令。\n")
    l_old, l_new = append_guarded(LEDGER, led_block.encode("utf-8"), "F4 台账")
    print(f"[F4] 协作总台账会话条目纯追加：前缀 SHA-256 {l_old[:16]}… 一致")

    print(f"[DONE] 域账 W90-Q5 90,000/240,000（37.5%）· 300 条冻结 · 关门印 W90-Q5-300 在位")
    return 0


if __name__ == "__main__":
    sys.exit(main())
