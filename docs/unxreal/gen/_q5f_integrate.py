# -*- coding: utf-8 -*-
"""AI-105 收官三落件整合器：主册增补卷（EOF 守卫式纯追加）+ 分工图收官登记块 + 台账封账条目
守卫：前缀 SHA-256 记录 + 回读断言 + 冲突区字节零触碰（只在 EOF 追加）
"""
import io, os, sys, hashlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
BOOK_DIR = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MASTER = os.path.join(BOOK_DIR, "CoRun Varix STAR II · Unxreal.md")
DIAGRAM = os.path.join(BOOK_DIR, "CoRun Varix STAR II · Unxreal · AI分工完成图.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
BOOK = os.path.join(BOOK_DIR, "AI-105 · Q5 · 500项深化册（B16–B40 · W90-Q5-301–800 · 域收官册）.md")

def guarded_append(path, block_bytes, tag):
    with io.open(path, "rb") as f:
        old = f.read()
    pre = hashlib.sha256(old).hexdigest()[:16]
    new = old + block_bytes
    assert new.startswith(old), f"{tag}: 前缀被改"
    with io.open(path, "wb") as f:
        f.write(new)
    with io.open(path, "rb") as f:
        back = f.read()
    assert back == new and back.startswith(old), f"{tag}: 回读不一致"
    print(f"{tag}: prefix {pre} -> {hashlib.sha256(back).hexdigest()[:16]} (+{len(block_bytes)}B)")

book = io.open(BOOK, "rb").read()

# 1) 主册增补卷（EOF 追加；主册当前处并行会话冲突态，本落件零触碰冲突区字节，追加于 EOF）
master_block = (
    "\n---\n\n## 增补卷 · AI-105 · W90-Q5 续卷收官册（B16–B40 · W90-Q5-301–800 · 500 条 · 域收官册）\n\n"
    "> AI-105 承办 · 域收官：段合流域账 800 条 / 240,000 / 240,000（100%）· 三印齐（判据印/满额印/收官印）。\n"
    "> 落件口径：主册当前处于他会话合并冲突态（<<<<<<< HEAD … ======= … >>>>>>> origin/main 固化于工作区），"
    "本卷按零触碰纪律以 EOF 纯追加落件、不改冲突区任何字节；冲突解决与他侧内容恢复归持锁会话（R-PROC-002 判例承 AI-26/102/105 首轮）。\n"
    "> 独立册与本卷逐字一致：《AI-105 · Q5 · 500项新功能续卷收官册》生成器 docs/unxreal/gen/_q5f_final.py 七断言 ALL PASS exit=0。\n\n"
).encode("utf-8") + book + b"\n"
guarded_append(MASTER, master_block, "MASTER")

# 2) 分工图收官登记块
diagram_block = """
---

## AI-105 · W90-Q5 UWP/MSIX 现代应用模型 续卷收官登记（2026-10-01 19:xx）

- **域收官**：《AI-105 · Q5 · 500项深化册（B16–B40 · W90-Q5-301–800 · 域收官册）》500 条 / 150,000 行落件；段合流域账 **800 条 / 240,000 / 240,000（100%）域封账**；三印齐（判据印/满额印/收官印）。
- **批面**：B16–B20 更新差分/卸载清理/许可商店/企业预配/诊断修复；B21–B25 完整性篡改响应/应用数据三仓/协议激活/共享剪贴板/通知磁贴；B26–B30 资产 DPI/本地化 MRT/启动性能/后台任务全族/应用服务；B31–B32 代表应用集成实案两批；B33 内核深接（pkgstore/fsview/reggate/kvsrv/syslogd/proc::job/task·power/signchain/secgate）；B34 安全审计与红线自证；B35–B36 回归对账联签（一致性套件 45 用例承 Q9-B06-02-J1 同构）；B37 性能收官；B38 可观测性收官；B39 域总对账；B40 域关门印。
- **机检**：生成器 docs/unxreal/gen/_q5f_final.py 七断言 ALL PASS exit=0（25 批在位/500 ID 连续 301–800 零跳号/500 主题唯一/每条详述 ≥300 字机检最短 453 字/主册 W90-Q5-301 起段追加前零命中防重/回读 500 表体/关门印 W90-Q5-800 在位）；批守恒 25×6,000 行全过。
- **内核锚定**：800/800（pkgstore·fsview·reggate·signchain·secgate·proc::job·kvsrv·syslogd·task·power / ktest M-Q5-### 全组 / kcheck 0 · 门禁铁值 ktest 3146）。
- **如实登记**：详述为判据深化账，实弹运行随 CW-07～12 波次窗闸门补测（开发期零 QEMU 零实机写）；主汇编册落件时实测处于他会话合并冲突态（HEAD/origin 双侧固化），本卷 EOF 纯追加零触碰冲突区，冲突解决归持锁会话；主册 112MB 超 blobs 上限沿 AI-68/71/86/96/99/109 先例本地落盘欠账登记。
- **关门印**：W90-Q5-801 起零外溢；他会话在途产物零触碰（分工图/台账纯追加）。
""".replace("\n", "\n").encode("utf-8")
guarded_append(DIAGRAM, diagram_block, "DIAGRAM")

# 3) 台账封账条目
ledger_block = """
---

### AI-105 会话登记 · W90-Q5 UWP/MSIX 续卷收官 500 项 · 域封账（2026-10-01 19:xx）

- **产出**：《AI-105 · Q5 · 500项深化册（B16–B40 · W90-Q5-301–800 · 域收官册）》500 条 / 150,000 行（25 批 × 20 条 × 6,000 行批守恒），每条附 ≥300 字定制详述（功能定位+完成标准，机检最短 453 字）；段合流域账 **800 条 / 240,000/240,000（100%）域封账**，三印齐，W90-Q5-801 起零外溢。
- **机检**：_q5f_final.py 七断言 ALL PASS exit=0；内核锚定 800/800；红线：涉写盘三重验证+dry-run+原子写、引导/NVRAM/内置盘零触碰、开发期零 QEMU 零实机写、实弹随闸门补测如实登记。
- **落件**：独立收官册 + 主汇编册卷末增补卷（EOF 纯追加）+ AI分工完成图收官登记块 + 本台账条目；生成器与数据卷 7 件（_q5f_final.py + _q5f_b1–b5.py）入库。
- **异常如实登记（R-PROC-002 三现）**：本会话落件时实测①分工完成图已被并行重写（AI-105 三处命中归零，登记块曾消失，本会话重登记）；②主汇编册处于他会话合并冲突态（377133 行 <<<<<<< HEAD / 418661 行 ======= / 418662 行 >>>>>>> origin/main 固化于工作区，尾侧段疑似双重编码乱码，分支 44/1 分叉、无 MERGE_HEAD）；③分工图/台账尾侧存在他会话乱码追加段。本会话一律零触碰、EOF 纯追加落件，冲突解决与乱码恢复归持锁会话/联席（再报 AI-92/AI-98）。
- **推送欠账**：主汇编册 ~112MB 超 GitHub blobs 100MB 上限，沿 AI-68/71/86/96/99/109 先例本地落盘不入 pathspec，欠账登记待镜像日合并清偿；独立册/生成器/分工图/台账走 api.github.com union 载体实推。
""".replace("\n", "\n").encode("utf-8")
guarded_append(LEDGER, ledger_block, "LEDGER")

print("THREE DEPOSITS ALL GUARDED-APPENDED")
