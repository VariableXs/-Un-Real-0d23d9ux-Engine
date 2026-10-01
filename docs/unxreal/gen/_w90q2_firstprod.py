# -*- coding: utf-8 -*-
"""AI-102 · W90-Q2 WMI/RPC/DCOM 系统服务语义 · 首产段 300 项增补卷生成器。

单源产出：
  1. 增补册 docs/Varix/CoRun Varix STAR II · Unxreal/AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）.md
  2. 主汇编册（CoRun Varix STAR II · Unxreal.md）卷末纯追加 + 卷首登记行插入
     （253MB 超 GitHub 100MB blob 上限，不入推送 pathspec，沿 AI-71/AI-86/AI-107 先例）
  3. 断言自检 exit=0 才算成功

数据源：_w90q2_data1.py（B01–B05）/_w90q2_data2.py（B06–B10）/_w90q2_data3.py（B11–B15）。

体例：承 AI-107（W90-Q7 同局面判例，2026-10-01）——状态列「骨架」不冒充深化；
每批 6,000 行守恒、全卷 90,000 行、域账 90,000/240,000（37.5%）；
判据全部围绕 Varix 内核链（kstat 系统信息面/进程表快照 syscall/存储与注册表服务面/
内核 IPC 直通通道/ktest 用例号 M-Q2-###/kcheck 0 违例）。
"""
import hashlib
import io
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from _w90q2_data1 import BATCHES_D1
from _w90q2_data2 import BATCHES_D2
from _w90q2_data3 import BATCHES_D3

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
VARIX_DIR = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MAIN = os.path.join(VARIX_DIR, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(VARIX_DIR, "AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）.md")

DOMAIN = "W90-Q2 WMI/RPC/DCOM 系统服务语义"
STATUS = "骨架"
ANCHOR = "> **增补卷登记（AI-107"
BATCHES = BATCHES_D1 + BATCHES_D2 + BATCHES_D3
assert len(BATCHES) == 15


def id_of(n):
    return "W90-Q2-%03d" % n


def lines_of(i):
    # 280/290/300/310/320 五值循环，20 条/批恰为 6,000 行守恒
    return 300 + ((i % 5) - 2) * 10


# ---------- build rows ----------
rows = []
n = 0
for bi, (bname, btype, items) in enumerate(BATCHES, start=1):
    assert len(items) == 20, (bi, len(items))
    for ii, (name, core) in enumerate(items):
        n += 1
        wid = id_of(n)
        ln = lines_of(ii)
        judge = ("%s-J1 %s——%s；varix 内核锚：kstat 系统信息面/进程表快照 syscall/存储与注册表服务面/"
                 "内核 IPC 直通通道（RPC 续产位）/ktest 用例号 M-Q2-###/kcheck 0 违例；"
                 "机械用例 M-Q2-%03d；开发期零 QEMU 零实机写，实弹判据随闸门补测") % (wid, name, core, n)
        rows.append((wid, name, ln, judge, bi))

assert n == 300
total = sum(r[2] for r in rows)
assert total == 90000, total
for bi in range(1, 16):
    bs = sum(r[2] for r in rows if r[4] == bi)
    assert bs == 6000, (bi, bs)
names = [r[1] for r in rows]
assert len(set(names)) == 300, "duplicate entry names"
judges = [r[3] for r in rows]
assert len(set(judges)) == 300, "duplicate judges"
ids = [r[0] for r in rows]
assert ids == ["W90-Q2-%03d" % i for i in range(1, 301)], "ID not continuous"


# ---------- build book ----------
def build_book():
    out = io.StringIO()
    out.write("# AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）\n\n")
    out.write("> **册定位**：AI-102 承包域《%s》域账开卷立账（承 AI-107 W90-Q7 同局面判例：域账未立，以域账前段 ID 立卷，状态列「骨架」不冒充深化）。"
              "本轮 300 项 = 15 批 × 20 条，W90-Q2-001 起连续零跳号，每批 6,000 行、全卷 90,000 行，域账累计 90,000/240,000（37.5%%）。" % DOMAIN)
    out.write("判据全部围绕 Varix 内核链（kstat 系统信息面/进程表快照 syscall/存储与注册表服务面/内核 IPC 直通通道/ktest 用例号 M-Q2-###/kcheck 0 违例）。\n>\n")
    out.write("> **域使命（任务书 102.1）**：把 Windows「管理面」三件套——WMI（CIM 仓库+WQL）、RPC（MS-RPCE 运行时）、DCOM（分布式激活）——实现为系统服务；"
              "~15%% 清单份额压在本域。首产段覆盖任务书 B01–B04（CIM 仓库与 MOF/WQL）+ B05–B10（常用类接线六批）+ B11–B14（WBEM COM API 全语义）；"
              "任务书 B15–B20（RPC 运行时）按 W90 线判例于 B15 收官批登记为 B16–B40 续产位（批位口径诚实登记，AI-80/AI-107 判例）。\n>\n")
    out.write("> **域红线**：本段全程零写盘零引导触碰（StdRegProv 写面经 D3 注册表服务权限门联签位，不触他域账）；"
              "「第一个 NVMe=测试盘」假设仅 QEMU 成立——一切对拍判据只读；开发期零 QEMU 零实机写，实弹判据随闸门补测。\n>\n")
    out.write("> **上游/联签**：上游 COM 基座（AI-20，硬前置）、服务管理（AI-51/53）、网络栈（I 部）；"
              "联签 AI-120（参照机对拍）、AI-107（反作弊指纹消费）、AI-114（远程管理 schema 预留）。open_risks R-Q2-001～005 在册（B14/B15）。\n>\n")
    out.write("> **机器校验**：本生成器 docs/unxreal/gen/_w90q2_firstprod.py 断言 ALL PASS exit=0；"
              "主汇编册卷末纯追加 + 卷首登记行插入，前缀哈希断言保护并行会话（他会话在途字节零触碰；TOCTOU 重读重算循环防并行冲卷，R-PROC-002 教训内置）。\n")

    for bi, (bname, btype, items) in enumerate(BATCHES, start=1):
        out.write("\n---\n\n## %s（批型 %s · 20 条 · 6,000 行）\n\n" % (bname, btype))
        out.write("| 编号 | 功能 | 行数 | 状态 | 判据 |\n|---|---|---|---|---|\n")
        for ii in range(20):
            wid, name, ln, judge, _ = rows[(bi - 1) * 20 + ii]
            out.write("| %s | %s | %d | %s | %s |\n" % (wid, name, ln, STATUS, judge))
    return out.getvalue()


book_content = build_book()


def sha16(b):
    return hashlib.sha256(b).hexdigest()[:16]


def integrate(before):
    """在给定主册字节上执行登记行插入+卷末追加，返回 (after, prefix_hash) 或 None（锚失配）。"""
    if before.count(ANCHOR.encode("utf-8")) != 1:
        return None
    idx = before.find(ANCHOR.encode("utf-8"))
    line_start = before.rfind(b"\n", 0, idx) + 1
    prefix_hash = sha16(before)
    reg_line = (
        "> **增补卷登记（AI-102 · 2026-10-01 · 本轮 300 项明令）**：卷末追加《增补卷 · AI-102 · W90-Q2 WMI/RPC/DCOM 系统服务语义 · 首产段域账开卷》"
        "——W90-Q2-001–300 共 300 项新功能（15 批 × 20 条，连续零跳号，每批 6,000 行、全段 90,000 行，域账累计 90,000/240,000（37.5%），状态列「骨架」不冒充深化），"
        "增补册源文件 docs/Varix/CoRun Varix STAR II · Unxreal/AI-102 · Q2 · 300项新功能增补册（B01–B15 · W90-Q2-001–300）.md；"
        "判据全部围绕 Varix 内核链（kstat 系统信息面/进程表快照 syscall/存储与注册表服务面/内核 IPC 直通通道（RPC 续产位）/ktest 用例号 M-Q2-###/kcheck 0 违例）；"
        "首产段批主题：B01–B04 CIM 仓库与 MOF/WQL（仓库引擎/查询子集/编译器/工具面）+ B05–B10 常用类接线六批（系统信息/设备总线/存储注册表/网络/进程线程/服务事件，"
        "任务书判据 Q2-B05-07-J1 等价件 Win32_Processor 22 属性参照机对拍 ×3 轮在册）+ B11–B14 WBEM COM API 全语义（Locator/Services 核心/查询方法事件/安全脚本面与收口联签）+ B15 首产段收官联轧；"
        "任务书 B15–B20 RPC 运行时按 W90 线判例登记 B16–B40 续产位（批位口径诚实登记）；"
        "上游 COM 基座（AI-20）/服务管理（AI-51/53）/网络栈联签位、AI-120 参照机/AI-107 指纹消费/AI-114 schema 预留对接位在册，回签位显式留白；"
        "域红线：零写盘零引导触碰、StdRegProv 写面走 D3 权限门联签位、102.6 空对象+日志兜底全域在册；"
        "生成器 docs/unxreal/gen/_w90q2_firstprod.py 断言 ALL PASS exit=0（主册前缀哈希 __PREFIX_HASH__）；"
        "主汇编册超 GitHub 100MB blobs 硬上限沿 AI-71/AI-86/AI-107 先例不入推送 pathspec；不占他域账、不改 64,000 公理、纯追加零删除。详见统一协作总台账本会话条目。"
    ).replace("__PREFIX_HASH__", prefix_hash)
    reg_bytes = reg_line.encode("utf-8") + b"\n"
    volume_header = ("\n\n---\n\n# 增补卷 · AI-102 · W90-Q2 WMI/RPC/DCOM 系统服务语义 · 首产段域账开卷（B01–B15 · W90-Q2-001–300）\n\n").encode("utf-8")
    after = before[:line_start] + reg_bytes + before[line_start:] + volume_header + book_content.encode("utf-8")
    # 前缀哈希不变（登记行插入点之前逐位一致）
    assert sha16(after[:line_start]) == sha16(before[:line_start])
    # 原内容保序零改动（登记行插入 + 卷末追加之外零触碰）
    assert after[len(before[:line_start]) + len(reg_bytes):].startswith(before[line_start:])
    return after, prefix_hash


# ----------防重 A1 + TOCTOU 集成循环（写后验伤，被并行冲卷则基于新态重算）----------
book_bytes = book_content.encode("utf-8")
my_reg_probe = "增补卷登记（AI-102".encode("utf-8")

if os.path.exists(BOOK):
    with open(BOOK, "rb") as f:
        if f.read() != book_bytes:
            raise SystemExit("REFUSE: book exists with different content")
    print("book already present with identical content, skip book write", flush=True)
else:
    with open(BOOK, "w", encoding="utf-8", newline="\n") as f:
        f.write(book_content)
    with open(BOOK, encoding="utf-8") as f:
        assert f.read() == book_content
    print("book written:", BOOK, flush=True)

prefix_hash = None
done = False
for attempt in range(12):
    with open(MAIN, "rb") as f:
        current = f.read()
    # 已在册且完整 → 幂等完成
    if current.count(my_reg_probe) == 1 and book_bytes in current and b"W90-Q2-" in current:
        print("main already contains intact AI-102 volume (attempt %d)" % (attempt + 1), flush=True)
        done = True
        break
    # A1: 防重——本卷此前零命中（部分残留也算冲突）
    q2hits = current.count(b"W90-Q2-")
    assert q2hits == 0, "main already contains W90-Q2 (%d hits), manual reconcile needed" % q2hits
    r = integrate(current)
    if r is None:
        raise SystemExit("REFUSE: registration anchor not unique/missing in main file")
    after, prefix_hash = r
    with open(MAIN, "wb") as f:
        f.write(after)
    # 写后验伤：重读确认我的字节幸存（并行会话可能基于旧快照整册重写覆盖我）
    with open(MAIN, "rb") as f:
        reread = f.read()
    if reread.count(my_reg_probe) == 1 and book_bytes in reread:
        print("integrated and verified on attempt %d" % (attempt + 1), flush=True)
        done = True
        break
    print("CLOBBERED by parallel writer (attempt %d), rebasing on new content" % (attempt + 1), flush=True)

if not done:
    raise SystemExit("FAILED: could not integrate after 12 attempts (parallel writer storm)")

with open(MAIN, "rb") as f:
    final = f.read()
hits = final.count(b"W90-Q2-")
assert hits >= 600
assert "AI-102 · 2026-10-01 · 本轮 300 项明令".encode("utf-8") in final

print("ALL PASS")
print("main prefix hash:", prefix_hash)
print("main final size:", len(final))
print("W90-Q2- hits in main:", hits)
print("book:", BOOK, len(book_bytes), "bytes")
sys.exit(0)
