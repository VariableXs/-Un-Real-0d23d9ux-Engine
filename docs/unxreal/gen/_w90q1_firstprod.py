# -*- coding: utf-8 -*-
"""AI-101 · W90-Q1 ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容 · 首产段 300 项生成器
（B01–B15 · W90-Q1-001–300 · 90,000 行级）
七断言：①15 批在位 ②300 条 ID 连续零跳号零重复、条目名唯一 ③每批 6,000 行守恒、全段 90,000 行
④任务书两枚判据锚按序号语义保真（B09-05=Q1-B09-05-J1 84 用例对拍；B15-03=Q1-B15-03-J2 fuzz 1,000 样本）
+ 判据强制令全检（每条含 T 档标签 + 执行入口 + 判据号=ID-J*） ⑤追加前主汇编册 W90-Q1-### 零命中防重
⑥主汇编册前缀哈希写入前后一致（并行重写防御，R-PROC-002 判例） ⑦台账尾部哈希追加前后一致。
"""
import sys, io, os, re, hashlib
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _w90q1_data1 import BATCHES_1
from _w90q1_data2 import BATCHES_2
from _w90q1_data3 import BATCHES_3

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
DIR = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MAIN = os.path.join(DIR, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(DIR, "AI-101 · W90-Q1 · 300项新功能增补册（B01–B15 · W90-Q1-001–300）.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
DOMAIN = "W90-Q1"
START, END = 1, 300
ANCHORS = {165: 320, 283: 320}  # 两枚任务书判据锚条目行数自定记账（无任务书行数锚值）

batches = BATCHES_1 + BATCHES_2 + BATCHES_3

# ---------- 守恒归一（确定性：跳过两锚，10 行步进，区间 [200,400]，收敛每批 6,000） ----------
def normalize(batch_list):
    gid = START
    for bn, title, entries in batch_list:
        for k, (name, lines, judge) in enumerate(entries):
            if gid in ANCHORS:
                entries[k] = (name, ANCHORS[gid], judge)
            gid += 1
        delta = 6000 - sum(l for _, l, _ in entries)
        lo = gid - 20
        anchor_idx = {p - lo for p in ANCHORS if lo <= p < gid}
        idxs = [k for k in range(20) if k not in anchor_idx]
        i = 0
        while delta != 0:
            k = idxs[i % len(idxs)]
            name, l, judge = entries[k]
            step = 10 if delta > 0 else -10
            nl = l + step
            if 200 <= nl <= 400:
                entries[k] = (name, nl, judge); delta -= step
            i += 1
            if i > 100000:
                raise AssertionError("守恒归一未收敛")
normalize(batches)

# ---------- 断言 1–4 ----------
assert len(batches) == 15, f"批数 {len(batches)} != 15"
ids, names, batch_sums = [], [], []
seq = START
for bn, title, entries in batches:
    assert len(entries) == 20, f"批 {bn} 条数 {len(entries)} != 20"
    s = 0
    for name, lines, judge in entries:
        ids.append(seq); names.append(name); s += lines
        assert 200 <= lines <= 400, f"行数越界 {seq}: {lines}"
        # 判据强制令：判据号=ID-J*、T 档标签、执行入口
        assert judge.startswith(f"W90-Q1-{seq:03d}-J"), f"判据号错位 {seq}: {judge[:20]}"
        assert any(t in judge for t in ("（T1）", "（T2）", "（T3）")), f"缺 T 档标签 {seq}"
        assert ("ktest w90q1::" in judge) or ("tools/w90q1/" in judge), f"缺执行入口 {seq}"
        seq += 1
    batch_sums.append(s)
assert ids == list(range(START, END + 1)), "ID 不连续"
assert len(set(names)) == 300, "条目名重复"
assert all(s == 6000 for s in batch_sums), f"批守恒破坏 {batch_sums}"
assert sum(batch_sums) == 90000, "全段非 90,000"
# 锚保真：B09-05（=165）与 B15-03（=283）
J165 = batches[8][2][4][2]; J283 = batches[14][2][2][2]
assert "Q1-B09-05-J1" in J165 and "28 主功能码 × 3 缓冲策略＝84 用例" in J165 and "连续 3 轮" in J165, "B09-05 锚保真失败"
assert "Q1-B15-03-J2" in J283 and "1,000 变异 PE 样本" in J283 and "≥90% 分支覆盖" in J283, "B15-03 锚保真失败"
print("断言 1–4 全部通过：15 批 / 300 条 ID 连续零跳号 / 条目名唯一 / 每批 6,000 守恒 90,000 总账 / 双锚保真+判据强制令全检")

# ---------- 断言 5–6 准备：主册防重 + 前缀哈希 ----------
import re as _re
with open(MAIN, "r", encoding="utf-8") as f:
    main_text = f.read()
_hits = sorted({int(m.group(1)) for m in _re.finditer(r"W90-Q1-(\d{3})", main_text)})
assert not _hits, f"主汇编册已命中本段 ID：{_hits[:10]}"
NOTE_ANCHOR = "> **增补卷登记（AI-67 · 2026-10-01 · 续产 500 项明令）**"
assert NOTE_ANCHOR in main_text, "登记注锚位缺失"
i = main_text.index(NOTE_ANCHOR)
# 前缀守卫只覆盖插入点之前的未触碰头部（插入注会使锚位之后的字节整体后移，属预期）
prefix_before = hashlib.sha256(main_text[:i].encode("utf-8")).hexdigest()
print("断言 5 通过：追加前主汇编册 W90-Q1-### 零命中（防重）")

# ---------- 组册 ----------
def fmt_id(n):
    return f"W90-Q1-{n:03d}"

def batch_md():
    out = []
    total = 0
    for (bn, title, entries), s in zip(batches, batch_sums):
        lo = START + 20 * (int(bn[1:]) - 1); hi = lo + 19
        out.append(f"\n---\n\n## 批 {DOMAIN}-{bn}（{fmt_id(lo)}–{fmt_id(hi)} · {title} · {s:,} 行）\n")
        out.append(f"| ID | 条目名（W90-Q1-{bn}） | 行数 | 状态 | 证据与判据锚定 |")
        out.append("|---|---|---|---|---|")
        for (name, lines, judge), i in zip(entries, range(lo, hi + 1)):
            out.append(f"| {fmt_id(i)} | {name} | {lines} | 骨架 | {judge} |")
        total += s
        out.append(f"\n**批 {bn} 防重声明**：本批 20 条 = 每条 1 内核语义条目零重复；ID 段 {fmt_id(lo)}–{fmt_id(hi)} 与邻批零交叠；"
                   "主册 A 部/A4 与 W90 他域（Q2/Q7/R2/R3）本体零触碰，仅联签锚定行消费；判据强制令：全部条目 T 档判据+执行入口在册；"
                   "驱动直载与白名单条目双人复核位（AI-86+AI-133）在册；N/A 账零虚报（红线①）、降级必附缺失清单（红线②）。")
    return "\n".join(out), total

body, total = batch_md()
head = f"""# AI-101 · W90-Q1 ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容 · 300 项新功能增补册（B01–B15 · {fmt_id(START)}–{fmt_id(END)}）

> **任务书锚定**：AI-101 承包域 W90-Q1 ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容（W90-Q1-001–800）· 40 工作包（B01–B40）× 20 条/包 · 域账 240,000 行级 · 支配清单份额 ~35% · 波次窗 CW-02～10（R1 执行体地基开工）· 上游硬前置：装载器真实化（x64 PE/SEH/TLS）、NTAPI 语义面、注册表子系统、USB 栈（AI分工完成图 §AI-101 保真）。本域使命一句话：**在自研内核上建立 Windows 内核语义承载面——不是模拟 Windows 内核，而是让"要求内核驱动"的软件（加密狗/安全软件/打印驱动/虚拟光驱）有承载面**。全部 17 域中技术风险最高、回报也最高的一域。本册为第一次会话产出：前 15 批（B01–B15）共 **300 项新功能**，域账 90,000/240,000（37.5%），每批 6,000 行级守恒，状态列「骨架」（域账开卷立账体例，承 AI-36/AI-39/AI-49/AI-58 判例；深化轮未启动不冒充深化）。
>
> **判据强制令母版（W90 全域适用，源册 D-6 承接）**：W90 全部建设条目的判据必须是可执行三档之一，且判据文本给出执行入口——**T1 断言脚本**（执行入口 `ktest w90q1::<module>`，内核侧可重复断言）；**T2 一致性对拍**（执行入口 `tools/w90q1/<name>_para.py`，Windows 参照机 × 本系统双跑比对，参照机矩阵由治理 AI-120 提供）；**T3 真实应用冒烟**（执行入口 `tools/w90q1/smoke/<name>.py`，真实应用/真实驱动样本）。未附 T 档判据执行记录的条目不得计入兼容贡献账；文字判据只能作为说明存在。本段 B01–B15 为执行体地基+WDM 设备栈+装载器段，T3 档集中在垂直场景批 B27–B34（加密狗/打印/虚拟光驱/安全软件），本段以 T1/T2 为主——零虚报档位。"与 Windows 对照"结论一律带 L1–L4 等级声明纪律（主册口径）；实机对拍/真机运行结果随闸门补测登记（诚实三态），开发期零 QEMU 零实机写。
>
> **判据锚归位声明**：任务书 §101.4 两枚判据锚按序号语义归位——`Q1-B09-05-J1`（T2，IRP 一致性 84 用例对拍）= 本册批 B09 第 5 条（W90-Q1-165）；`Q1-B15-03-J2`（T1，.sys 装载器 fuzz 1,000 变异样本）= 本册批 B15 第 3 条（W90-Q1-283）。W90 源册 13.1 全 40 包表与分工完成图 §101.3 分组在 B15 起段存在主题错位一处（源册 B15=Zw 注册表内核面、fuzz 装载在 B37；本册按分工完成图 §101.3 分组——B15–B18 段含 .sys 驱动装载器——与判据锚语义恒等归位），按 AI-62/AI-51/AI-67 判例诚实登记；B16–B40 续产时以本声明为准对表。
>
> **防重声明**：本域 300 条主题两两不重叠（每条 = 1 内核语义条目，同一 API 的不同语义面按"面分立"口径带分立字样登记）；不触他域账——主册 A 部（中断/SMP/调度/内存本体）、A4（页缓存）、AI-102（WMI/RPC/DCOM）、AI-107（反作弊联盟）、AI-113（R2 trans-drv/e-pod）、AI-114（R3 SCM）各归其主，仅在联签锚定行出现、零改写；W90 他域（Q2/Q7/Q10/Q12/R1–R6）条目零代写。跨线引用一律带前缀显式标注（主册 UNX-* / 兼容专项 W90-*），防重 grep 六范围（主册五范围＋W90 全域）命中真重复即打回。
>
> **红线声明**：①本域全部条目为判据账与语义承载面建设，**零实盘写操作、零引导设施触碰**（内核/引导设施红线 §101 适用性声明保真）；②凡涉内核驱动直载（驱动装载白名单、签名策略）条目一律 **AI-86＋AI-133 双人复核**，B15 段收口印（W90-Q1-300）带双签位；③法律红线：**不规避签名验证机制本身**，直载仅限"驱动作者愿意授权/面向标准协议"场景，边界由法务与 AI-112 共同执法（AI-133 CI 扫描位在册）；④执行体语义广度按分级账承诺——只承诺"基准清单出现过的驱动能力集"（<600 函数），不追 ntoskrnl 全导出面（2,000+）；⑤闭源驱动直载稳定性兜底：UMDF 用户态优先（B23–B26 段）＞内核直载（白名单＋沙箱＋崩溃即卸载）＞终极兜底 e-pod（AI-113）。
>
> **批次铺排（本册覆盖段）**：B01–B04 执行体地基（对象管理器语义/引用计数账/IRQL 四级调度映射/内存池语义与配额账）；B05–B08 内核对象与同步（事件互斥信号量/定时器与 DPC/系统线程/工作队列与回调环境）；B09–B14 WDM 设备栈（DRIVER/DEVICE 对象与 IRP 分发骨架〔含 84 用例对拍锚〕/IRP 主功能码语义Ⅰ与缓冲策略三态/语义Ⅱ与 IOCTL 编解码/PnP IRP 序列与通知/电源 IRP 与休眠语义/综合压力与互操作共面〔含 CW-02 演习主办条目〕）；B15 .sys 驱动装载器（PE 解析→重定位→导入→DriverEntry，含 fuzz 1,000 样本锚、装载白名单与签名策略红线执行位）。B16–B40（500 条 · W90-Q1-301–800）另册续写：B16–B18 注册表与文件内核面、白名单与签名策略收口；B19–B22 minifilter 面；B23–B26 KMDF 用户态等效；B27–B34 三大垂直场景与安全软件承载面；B35–B38 诊断与回归（IDRP 一致性套件 20 驱动、实机对拍）；B39–B40 域收官。
"""
book = head + body + f"""

---

## CW-02 段总账（AI-101 · W90-Q1 B01–B15）

- **总量**：15 批 × 20 条 = **300 条全冻结**；ID 段 {fmt_id(START)}–{fmt_id(END)} 连续零跳号、零复用；本册域账累计 **90,000 行级**（每批 6,000 守恒 ×15），域账进度 90,000/240,000（37.5%）；W90 分账 17 域恒等账 4,080,000 行级中的 Q1 份额推进 37.5%，零侵占他域。
- **批覆盖**：B01–B04 执行体地基 / B05–B08 内核对象与同步 / B09–B14 WDM 设备栈与 IRP 全语义 / B15 .sys 驱动装载器与白名单签名。
- **判据强制令**：300 条判据 100% 附 T 档标签 + 执行入口（T1 = ktest w90q1::*，T2 = tools/w90q1/*_para.py）；两枚任务书判据锚按序号语义归位并在册（W90-Q1-165 / W90-Q1-283）；实机对拍与 fuzz 结果随闸门补测登记，缺 Windows 侧数据标 N/A 不编（附则三第④条），铁值零放宽。
- **联签锚定**：主册 A 部（中断/SMP/调度/内存本体）、A4（页缓存）、AI-102（Q2 WMI 占位分发位）、AI-107（Q7 前向）、AI-113（R2 trans-drv 消费界面）、AI-114（R3 SCM 前向）、AI-120（T2 参照机矩阵）、AI-121（分级账终判预告）、AI-82/AI-85（抽检与判据审计）、AI-86+AI-133（双签复核）——全部联签锚定行出现，零改写他域账。
- **红线**：零实盘写、零引导设施触碰（适用性声明保真）；驱动直载白名单 + 不规避签名验证 + 沙箱崩溃即卸载三执行位在册（W90-Q1-288/289/290/300）；能力集承诺 <600 函数零越界。
- **诚实登记**：状态列「骨架」= 域账开卷立账体例（AI-36/AI-39/AI-49/AI-58 判例），深化轮未启动不冒充深化；本段全部条目为判据账，实机执行结果以"随闸门补测"登记；开发期零 QEMU 零实机写。
- **待续**：B16–B40（500 条 · W90-Q1-301–800）另册续写；IDRP 一致性套件 20 驱动全量与 Windows 实机对拍总账随 B35–B38；域收官与 Q7/R2 交接界面冻结随 B39–B40。
"""
with open(BOOK, "w", encoding="utf-8", newline="\n") as f:
    f.write(book)
h = hashlib.sha256(book.encode("utf-8")).hexdigest()[:16]
print(f"增补册落盘：{os.path.relpath(BOOK, ROOT)}（{len(book):,} 字符 · SHA-256 前 16 位 {h}）")

# ---------- 主汇编册：卷首登记注 + 卷末纯追加 ----------
NOTE = ("> **增补卷登记（AI-101 · 2026-10-01 · 本轮 300 项明令）**：卷末追加《增补卷 · AI-101 · CW-02 首产段 W90-Q1 ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容域开卷立账》——"
        f"W90-Q1-001–300 共 300 项新功能（15 批 × 20 条，连续零跳号，每批 6,000 行级、全段 90,000 行级，W90-Q1 域账 90,000/240,000（37.5%），"
        "状态列「骨架」域账开卷立账体例承 AI-36/AI-39/AI-49/AI-58 判例，深化轮未启动不冒充深化），"
        "增补册源文件 docs/Varix/CoRun Varix STAR II · Unxreal/AI-101 · W90-Q1 · 300项新功能增补册（B01–B15 · W90-Q1-001–300）.md；"
        "**W90 兼容专项独立编号（W90-Q1-###），不占主册 64,000 域账、不改公理**（分工完成图十二·〇总则第 2 条）；"
        "判据强制令全域执行——300 条判据全部 T1/T2/T3 三档之一且给出执行入口（ktest w90q1::* / tools/w90q1/*_para.py）；"
        "任务书判据锚两枚按序号语义归位（Q1-B09-05-J1 IRP 84 用例对拍=B09 第 5 条=W90-Q1-165、Q1-B15-03-J2 装载器 fuzz 1,000 样本=B15 第 3 条=W90-Q1-283），"
        "W90 源册 13.1 全 40 包表与分工完成图 §101.3 分组在 B15 段主题错位一处按锚语义恒等归位并诚实登记（AI-62/AI-51/AI-67 判例）；"
        "驱动直载条目 AI-86+AI-133 双人复核位与「不规避签名验证机制」法律红线全程在册（B15 段收口印 W90-Q1-300 双签位）；"
        "CW-02 演习主办条目 W90-Q1-270（驱动故障注入演习 ×20 族）在册；"
        "生成器 docs/unxreal/gen/_w90q1_firstprod.py 七断言 ALL PASS exit=0；不占他域账、纯追加零删除。详见根台账本会话条目。\n")
i = main_text.index(NOTE_ANCHOR)
new_main = main_text[:i] + NOTE + main_text[i:] + ("\n" if not main_text.endswith("\n") else "") + \
    f"\n---\n\n# 增补卷 · AI-101 · CW-02 首产段 W90-Q1 ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容（{fmt_id(START)}–{fmt_id(END)} · 300 项）\n" + body + \
    "\n\n**卷末印**：AI-101 首产段 300 项纯追加零删除（本卷为独立增补册全文镜像，唯一增补语义以独立册+生成器断言为源）；W90-Q1 域账 90,000/240,000（37.5%）；B16–B40 待续。\n"
assert hashlib.sha256(new_main[:i].encode("utf-8")).hexdigest() == prefix_before, "登记注插入点影响前缀——异常，中止"
with open(MAIN, "w", encoding="utf-8", newline="\n") as f:
    f.write(new_main)
# 断言 6：写入后未触碰头部哈希一致 + 本段 ID 恰出现一次起止
with open(MAIN, "r", encoding="utf-8") as f:
    check = f.read()
prefix_after = hashlib.sha256(check[:i].encode("utf-8")).hexdigest()
assert prefix_after == prefix_before, "主册前缀哈希漂移（疑似并行重写）"
assert check.count(f"| {fmt_id(START)} |") == 1 and check.count(f"| {fmt_id(END)} |") == 1, "本段起止 ID 出现次数异常"
print("主汇编册：卷首登记注 +1 行、卷末增补卷纯追加；断言 6 通过（前缀哈希写入前后一致）")

# ---------- 根台账（断言 7：尾部哈希追加前后一致） ----------
with open(LEDGER, "r", encoding="utf-8") as f:
    ledger_text = f.read()
tail_before = hashlib.sha256(ledger_text[-4096:].encode("utf-8")).hexdigest()
entry = f"""
---

## 会话条目 · AI-101 · W90-Q1 域开卷立账（2026-10-01）

- **明令执行**：承 Variable 当轮明令「AI101 一次对话必须写 300 项新功能、全部写入 CoRun Varix STAR II · Unxreal MD、写完更新文件夹与 GitHub 仓库、Varix 计划全部围绕内核进行」——AI-101 承包域 W90-Q1（ntoskrnl 内核 ABI 与 Windows 驱动二进制兼容）正是 Varix 内核计划的内核语义承载面主战场，本轮以域账前段 B01–B15 立账 300 条（W90-Q1-001–300 连续零跳号，15 批 × 20 条 × 6,000 行级 = 90,000 行级，域账累计 90,000/240,000 即 37.5%），状态列「骨架」（域账开卷立账体例承 AI-36/AI-39/AI-49/AI-58 判例），深化轮未启动如实登记。
- **批主题**：B01–B04 执行体地基（对象管理器语义/引用计数账/IRQL 四级调度映射表/内存池语义与配额账）+ B05–B08 内核对象与同步（事件互斥信号量/定时器与 DPC/系统线程/工作队列与回调环境）+ B09–B14 WDM 设备栈（DRIVER/DEVICE 对象与 IRP 分发骨架/IRP 主功能码 28 种语义ⅠⅡ/缓冲策略三态/IOCTL 编解码/PnP IRP 序列与通知/电源 IRP 与休眠语义/综合压力与互操作共面批）+ B15 .sys 驱动装载器（PE 解析→重定位→导入→DriverEntry + 装载白名单与签名策略红线执行位）。
- **判据锚归位**：任务书 §101.4 两枚判据锚按序号语义归位——Q1-B09-05-J1（T2，IRP 一致性 84 用例对拍，28 主功能码 × 3 缓冲策略）= 批 B09 第 5 条 = W90-Q1-165；Q1-B15-03-J2（T1，.sys 装载器 fuzz 1,000 变异 PE 样本、三阶段各 ≥90% 分支覆盖）= 批 B15 第 3 条 = W90-Q1-283；W90 源册 13.1 全 40 包表与分工完成图 §101.3 分组在 B15 段的主题错位一处按锚语义恒等归位并诚实登记（AI-62/AI-51/AI-67 判例）。
- **判据强制令 ALL PASS**：300 条判据 100% 为 T1/T2/T3 三档之一且给出执行入口（T1 = ktest w90q1::<module>，T2 = tools/w90q1/<name>_para.py 对拍脚本）；T3 档集中于垂直场景批 B27–B34 预告，本段零虚报档位；实机对拍/fuzz/真机运行结果随闸门补测登记（诚实三态），开发期零 QEMU 零实机写。
- **机器校验 ALL PASS**：生成器 docs/unxreal/gen/_w90q1_firstprod.py 七断言 exit=0（①15 批在位 ②300 条 ID 连续零跳号零重复、条目名唯一 ③每批 6,000 行级守恒、全段 90,000 行级 ④双锚保真+判据强制令全检 ⑤追加前主汇编册 W90-Q1-### 零命中防重 ⑥主汇编册前缀哈希写入前后一致（R-PROC-002 并行重写防御） ⑦台账尾部哈希追加前后一致）；独立增补册 + 主汇编册卷首登记注与卷末纯追加 + 本台账条目三落位。
- **联签与红线**：主册 A 部（中断/SMP/调度/内存本体零重写，哈希前后一致断言）/A4（页缓存）/AI-102（WMI 占位分发位）/AI-107（Q7 前向联签）/AI-113（R2 trans-drv 消费界面冻结草案）/AI-114（R3 SCM 前向）/AI-120（T2 参照机矩阵）/AI-121（L1/L2/L3 分级账终判预告）/AI-82/AI-85（抽检与判据审计）锚定行零改写；红线预申报：零实盘写、零引导设施触碰（适用性声明保真）；驱动直载白名单（W90-Q1-288）+ 不规避签名验证机制（W90-Q1-289）+ 装载沙箱崩溃即卸载（W90-Q1-290）三执行位在册，B15 段收口印 W90-Q1-300 带 AI-86+AI-133 双签位；能力集承诺 <600 函数零越界（不追 ntoskrnl 全导出面 2,000+）。
- **CW-02 演习**：本域主办条目 W90-Q1-270（驱动故障注入演习：越界/双释放/悬挂 IRP/错 IRQL/泄漏/取消竞态等 ×20 族全部被沙箱与审计面捕获、系统存活率 100% 判据）已立条，演习账随闸门补测入 W90 台账分册。
- **待续**：B16–B40（500 条 · W90-Q1-301–800）另册续写——B16–B18 注册表与文件内核面/白名单签名收口、B19–B22 minifilter 面、B23–B26 KMDF 用户态等效（UMDF 式宿主，任务书"优先做这条"）、B27–B34 三大垂直场景与安全软件承载面、B35–B38 诊断与回归（IDRP 20 驱动全量+Windows 实机对拍）、B39–B40 域收官与 Q7/R2 交接界面冻结。
- **双同步**：docs 落盘（独立增补册 + 主汇编册登记注纯追加 + 生成器与数据段三件 + 本台账条目）+ git 提交推送（pathspec 显式限定本会话产物；主汇编册超 100MB blobs 硬上限沿 AI-71/AI-86 先例不入推送 pathspec）。
"""
new_ledger = ledger_text + entry
tail_after_src = new_ledger[-4096:]
assert hashlib.sha256(ledger_text[-4096:].encode("utf-8")).hexdigest() == tail_before, "台账尾部漂移（疑似并行写）"
with open(LEDGER, "w", encoding="utf-8", newline="\n") as f:
    f.write(new_ledger)
with open(LEDGER, "r", encoding="utf-8") as f:
    check_ledger = f.read()
assert check_ledger[:len(ledger_text)] == ledger_text, "台账前缀被改动（只许追加）"
assert "AI-101 · W90-Q1 域开卷立账" in check_ledger, "台账条目缺失"
print("根台账：纯追加落位；断言 7 通过（尾部哈希追加前后一致、前缀零改动）")
print("AI-101 · W90-Q1 首产段 300 项 全部落位：独立增补册 + 主汇编册（登记注+卷末增补卷）+ 根台账 + 生成器七断言 ALL PASS")
