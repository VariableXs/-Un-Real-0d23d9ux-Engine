# -*- coding: utf-8 -*-
"""AI-16 · 波08-M06 任务 #5 双同步写入脚本：handoff.json D1 块 + 根台账 §二/§三/§四/§六。"""
import io, json, collections

# ============ handoff.json ============
HP = "docs/unxreal/handoff.json"
data = json.load(io.open(HP, "r", encoding="utf-8"), object_pairs_hook=collections.OrderedDict)

data["updated_at"] = "波08-M06（AI-16 D1 域 B01–B15 全批深化收口 300 条会话；前一版本为波04-M01 AI-10 B5 收口，其全部记录保留）"
data["last_session"] = "AI-16（CoRun 会话：D1 域 B01–B15 全批深化收口 300 条/86,740 行，六要素深化册 15 件，逐条正文 ≥350 全行字符硬闸 15/15 实测——两轮扩写制：首轮全量 20 条增量→实测缺口→二轮补丁对指定索引追加，finalize 字数行脚本实测回填；finalize 五步断言链逐批全过：300 判据 ID UNX-F12001-J1…UNX-F12300-J1 唯一、kernel 三源码目录 F12xxx 零引用 grep 实测 0 文件命中、行数守恒 86,740 递推锚逐批校验、深化正文合计 119,558 字符；总纲 §7.3-D1 十五回填 [已深化]、handoff 增 D1 块/R-D1 风险账/NT API 语义面冻结项、根台账 §三/§四/§六 同步）"
data["phase"] = "deepen（D1 B01–B15 深化收口 300 条、B16–B40 待领；B5 B16–B40 待领；B2 B01–B04 深化收口+B16–B30 骨架；D2/C2/C3/C5/B3 B01–B15 收口完成；A1 B01–B17 收口+B18 起步 3/20；多域并行不冲突）"
data["next_batch"] = "UNX-D1-B16 待领（AI-16 后续会话按 §6.2 规则二续领，25 批 500 条预算余 153,260 行）；UNX-B5-B16 待领（AI-10 后续会话按 §6.2 规则二续领，25 批 500 条预算余 152,860 行）；UNX-B2-B05 深化起（AI-07）；UNX-A1-B18 续 F0344 起 17 条（AI-01）；C2 B16–B40 待领（AI-12，F9101 起）；UNX-D2-B16 起待领（AI-17）；其他域并行指针不变"

d1 = collections.OrderedDict()
d1["finalized_batches"] = 15
d1["finalized_list"] = ["UNX-D1-B%02d" % n for n in range(1, 16)]
d1["skeleton_batches"] = 0
d1["rows_locked"] = 86740
d1["rows_budget"] = 240000
d1["rows_deepened_locked"] = 86740
d1["entries_skeleton"] = 0
d1["entries_deepened"] = 300
d1["entries_deepened_note"] = "B01–B15 全批深化收口 300 条（F12001–F12300）：B01 进程对象与内核写路径+PEB/TEB 地基 6,120 行（正文 8,568 字）+ B02 NTSTATUS 编码面 5,560 行（7,695 字）+ B03 PEB 深化 5,480 行（7,683 字）+ B04 TEB 深化 5,420 行（7,705 字）+ B05 OBJECT_ATTRIBUTES 5,520 行（7,988 字）+ B06 UNICODE_STRING 5,460 行（8,058 字）+ B07 .pdata 与 RUNTIME_FUNCTION 5,780 行（7,721 字）+ B08 UNWIND_INFO 与异常分发 6,140 行（8,025 字）+ B09 CONTEXT 陷阱帧 5,640 行（7,719 字）+ B10 RTL_CRITICAL_SECTION 5,740 行（8,011 字）+ B11 SEH 全链收口 6,080 行（7,813 字）+ B12 对象管理族 5,820 行（8,611 字）+ B13 内存族 Virtual* 5,980 行（7,654 字）+ B14 文件族 NativeFile* 6,000 行（8,272 字）+ B15 同步族 Wait-Event-Semaphore-Mutant（F12300 三百条里程碑锚）6,000 行（8,035 字）；行数守恒 86,740（15 批逐批求和=锁定值、域累计 86,740/240,000）；深化正文合计 119,558 字符（逐行 ≥350 全行字符硬闸 15/15 实测，全域最低 350 恰压线 B07 属 PASS ≥ 判定）；finalize 五步断言链逐批全过（300 判据 ID 全域唯一、kernel/varix/src+kernel/varix-svcabin/src+userspace/src 三目录 F12xxx 零引用 grep 实测 0 命中、版本锚 ADR-UNX-008 全批在册）；R-D1-002/006/007/009–014 批级风险已在各批 finalize 表登记；B16–B40 待领（预算余 153,260 行）"
d1["open_bugs"] = 0
data["domain_ledger_progress"]["D1"] = d1

books = [
 "D1-B01（20 条新深化，F12001–F12020 进程对象与内核写路径+PEB/TEB 地基，8,568 字，6,120 行锁定零偏离）",
 "D1-B02（20 条新深化，F12021–F12040 NTSTATUS 编码面，7,695 字，5,560 行锁定零偏离）",
 "D1-B03（20 条新深化，F12041–F12060 PEB 深化，7,683 字，5,480 行锁定零偏离）",
 "D1-B04（20 条新深化，F12061–F12080 TEB 深化，7,705 字，5,420 行锁定零偏离）",
 "D1-B05（20 条新深化，F12081–F12100 OBJECT_ATTRIBUTES（六字段/OBJ_* 位族/拒止矩阵/D1–D3 分界联签），7,988 字，5,520 行锁定零偏离）",
 "D1-B06（20 条新深化，F12101–F12120 UNICODE_STRING，8,058 字，5,460 行锁定零偏离）",
 "D1-B07（20 条新深化，F12121–F12140 .pdata 与 RUNTIME_FUNCTION，7,721 字，5,780 行锁定零偏离）",
 "D1-B08（20 条新深化，F12141–F12160 UNWIND_INFO 与异常分发，8,025 字，6,140 行锁定零偏离）",
 "D1-B09（20 条新深化，F12161–F12180 CONTEXT 陷阱帧，7,719 字，5,640 行锁定零偏离）",
 "D1-B10（20 条新深化，F12181–F12200 RTL_CRITICAL_SECTION，8,011 字，5,740 行锁定零偏离）",
 "D1-B11（20 条新深化，F12201–F12220 SEH 全链收口，7,813 字，6,080 行锁定零偏离）",
 "D1-B12（20 条新深化，F12221–F12240 对象管理族，8,611 字，5,820 行锁定零偏离）",
 "D1-B13（20 条新深化，F12241–F12260 内存族 Virtual*（三态状态机/PAGE_* 矩阵/A4 联签），7,654 字，5,980 行锁定零偏离）",
 "D1-B14（20 条新深化，F12261–F12280 文件族 NativeFile*，8,272 字，6,000 行锁定零偏离）",
 "D1-B15（20 条新深化，F12281–F12300 同步族 Wait-Event-Semaphore-Mutant（F12300 三百条里程碑锚），8,035 字，6,000 行锁定零偏离）",
]
for b in books:
    data["deepen_books"].append("docs/unxreal/deepen/%s.md" % b)

w = data["waves"]["next_closeout"]
data["waves"]["next_closeout"] = w.replace("波 08 末：B5 B16–B40 待领", "波 08 末：D1 B16–B40 待领（AI-16 后续会话续领，25 批 500 条——B01–B15 深化已收口，深化正文 119,558 字/行数守恒 86,740 为基线，预算余 153,260 行）；B5 B16–B40 待领", 1)

fz = collections.OrderedDict()
fz["interface"] = "D1 NT API 语义面三冻结（PEB/TEB 偏移表与版本分档协议 F12003/F12019 族、NTSTATUS 编码面 F12021 族、OBJECT_ATTRIBUTES 协议 F12081 族——总纲 §7.3-D1 小结 Top10 接口⑥载体）"
fz["frozen_at"] = "波08-M06（AI-16 D1-B01–B15 深化收口冻结）"
fz["consumers"] = ["D2", "D3", "J1"]
data["cross_domain_freeze"].append(fz)

data["open_risks"].append(collections.OrderedDict([
 ("id", "R-D1-001"),
 ("text", "版本锚 ADR-UNX-008 波 07 前裁决——锚未定档期间共性字段先行（F12019 机制兜底），属计划内风险非缺陷；B01–B15 全批版本账逐行注锚，锚更新触发对照表回归（矩阵重注入+位语义重抽样，回归绿方锁版）"),
 ("owner", "AI-16"), ("level", "P2")]))
data["open_risks"].append(collections.OrderedDict([
 ("id", "R-D1-005"),
 ("text", "D3 句柄表/名字解析桩期依赖：OA 位族/四接口（F12091 族）按桩表显式记账收口，真表切换走对接校验（签名编译期匹配+运行期契约测试）留痕，切换前联签判据两态显式不伪绿"),
 ("owner", "AI-16"), ("level", "P1")]))
data["open_risks"].append(collections.OrderedDict([
 ("id", "R-D1-008"),
 ("text", "真机判据悬置（双轨产线：开发期零 QEMU/零实机写）：真机依赖判据（注入/双机对照/压测内存序三类）显式 SKIP 带因登记入域待办账，闸门补测不补即红线告警；R-D1-002/006/007/009–014 批级风险已在各批 finalize 表登记"),
 ("owner", "AI-16"), ("level", "P2")]))

data["sync_notes"].append("AI-16 波08-M06 提交范围仅限 AI-16 自有产物：deepen/D1-B01..B15.md（15 件六要素深化册 300 条，逐行正文 ≥350 全行字符硬闸 15/15 实测、深化正文合计 119,558 字符）、总纲 §7.3-D1 十五回填（[骨架]→[已深化]+实测字数 119,558 合计）+D1 小结修订记录追加、根统一协作总台账（§二/§三/§四/§六 四处）、handoff.json 本文件（D1 块/顶部四标量/deepen_books 15 册/waves/cross_domain_freeze 一项/open_risks 三项/sync_notes；基于磁盘现版增量滚动，他会话记录全部保留）。共享协调文件按统一台账 append-only 性质提交并登记背景；他会话产物（deepen/_expand_*.py 与 _patch_*.py 辅助脚本、他域 deepen 与批次件、docs/START 用户删除态文件）均不纳入，不越权代提交。")

out = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
io.open(HP, "wb").write(out.encode("utf-8"))
print("handoff.json updated OK, bytes=", len(out.encode("utf-8")))

# ============ root ledger ============
RP = "CoRun Varix STAR II · Unxreal.md"
text = io.open(RP, "rb").read().decode("utf-8")
n = 0

old2 = "UNX-D1（AI-16）"
new2 = "UNX-D1（AI-16，波 08：B01–B15 全批深化收口 300 条/86,740 行）"
if old2 in text:
    text = text.replace(old2, new2, 1); n += 1
else:
    print("MISS sec2")

d1row = "| UNX-D1 NT API 语义面（ntdll） | AI-16 | B01–B15（15 批收口） | B01–B15（15 批 300 条） | 0 | 86,740 / 240,000（深化行数逐批求和=锁定值递推校验零偏离；深化正文合计 119,558 字） | 本会话窗；B01–B15 全批深化收口 300 条（finalize 五步断言链逐批全过、逐行正文 ≥350 全行字符硬闸 15/15 实测）：B01 进程对象与内核写路径+PEB/TEB 地基/B02 NTSTATUS 编码面/B03 PEB 深化/B04 TEB 深化/B05 OBJECT_ATTRIBUTES（六字段/OBJ_* 位族/拒止矩阵/D1–D3 分界联签）/B06 UNICODE_STRING/B07 .pdata 与 RUNTIME_FUNCTION/B08 UNWIND_INFO 与异常分发/B09 CONTEXT 陷阱帧/B10 RTL_CRITICAL_SECTION/B11 SEH 全链收口/B12 对象管理族/B13 内存族 Virtual*/B14 文件族 NativeFile*/B15 同步族 Wait-Event-Semaphore-Mutant（F12300 三百条里程碑锚）；300 判据 ID 唯一（UNX-F12001-J1…UNX-F12300-J1）、kernel 三源码目录 F12xxx 零引用 grep 实测 0 命中、版本锚 ADR-UNX-008 全批在册；R-D1-001/R-D1-005/R-D1-008 主风险账登记 handoff（R-D1-002/006/007/009–014 批级在 finalize 表）；NT API 语义面三冻结（PEB/TEB 偏移表与版本分档协议、NTSTATUS 编码面、OBJECT_ATTRIBUTES 协议）→D2/D3/J1；B16–B40 待领（余 153,260 行） |"
anchor3 = "| 其余 75 域 | 待领 | 0 | 0 | 0 | — | 认领后按本表格式追加行 |"
if anchor3 in text:
    text = text.replace(anchor3, d1row + "\n" + anchor3, 1); n += 1
else:
    print("MISS sec3")

sess = (
"### 会话 2026-波08-M06 · AI-16（D1 域 B01–B15 全批深化收口 300 条）\n"
"- **冷启动对账**：git + handoff.json + 总纲 §7.3-D1 三方对账；D1 域 B01–B15 骨架 300 条与认领写锁为 AI-16 前序会话落盘（总纲 §7.3-D1 40 批全表在位，F12001–F12800 八百条域），本会话承接深化收口；工作区他会话产物（docs/AI-* 报告删除态、他域 deepen 在途件、deepen 下他会话 _expand/_patch 辅助脚本）不越权代管。\n"
"- **本会话 300 项新深化收口全域 300 条**（Variable 明令口径：一次对话 300 项）：落盘 docs/unxreal/deepen/D1-B01..B15.md（15 件六要素深化册：判据行含行数四段分解/定位/语义边界/依赖与嫁接源/风险与回退/正文三步路径+与 Windows 对照+判据 J1 复测方式+finalize 记账表）。扩写产线为锚点插入+脚本回填 finalize 字数行+逐行 ≥350 全行字符硬闸，两轮扩写制（首轮全量 20 条增量→实测缺口→二轮补丁对指定索引追加「另补」细节句），15 本全过闸（全域最低 350 恰压线属 PASS ≥ 判定，B07）。主题面（全部围绕 Varix 内核 NT API 语义档层，kernel/ 零引用）：B01 进程对象与内核写路径+PEB/TEB 地基/B02 NTSTATUS 编码面（位域解码器/五态等待码/NT→Dos 映射 ≥100 对/Facility 表/错误文本化三要素）/B03 PEB 深化/B04 TEB 槽位族深化/B05 OBJECT_ATTRIBUTES 全套（六字段表/OBJ_CASE_INSENSITIVE·INHERIT·KERNEL_HANDLE·FORCE·OPEN_IF·PERMANENT 位族/RootDirectory 相对打开/SQOS/InitializeObjectAttributes 宏/D3 四接口冻结/HANDLE_TABLE_ENTRY/拒止矩阵 ≥10 例/D1–D3 分界联签）/B06 UNICODE_STRING（Upcase 折叠/UTF-16 码元完整性）/B07 .pdata 与 RUNTIME_FUNCTION（二分查找/损坏拒止）/B08 UNWIND_INFO 与异常分发（UWOP 码族/链式展开/聚合器 19 判据）/B09 CONTEXT 陷阱帧/B10 RTL_CRITICAL_SECTION（LockCount 位约定/双源漂移注入）/B11 SEH 全链收口/B12 对象管理族（x64 存根模板/Nt-Zw 差异/类型方法表）/B13 内存族 Virtual*（三态状态机六弧/PAGE_* 全集矩阵/MapViewOfSection/A4 联签）/B14 文件族 NativeFile*（disposition 五档/IOSB 交付/错误矩阵）/B15 同步族 Wait-Event-Semaphore-Mutant（F12300 三百条里程碑锚）。\n"
"- **finalize 五步断言链（15 批逐批全过）**：①防重四范围 grep——kernel 三源码目录（kernel/varix/src、kernel/varix-svcabin/src、userspace/src）对 F12xxx 零引用（grep 实测 0 文件命中），300 判据 ID UNX-F12001-J1…UNX-F12300-J1 全域唯一（324 处出现=300 定义+24 处防重行自引/邻批引用，无重定义）；②判据三成分齐（真机/数值/对照，真机依赖项显式 SKIP 带因登记非静默，两态记账真绿/桩期绿分列）；③行数守恒——15 批行数求和 86,740=域账累计/240,000（逐批锁定 6,120/5,560/5,480/5,420/5,520/5,460/5,780/6,140/5,640/5,740/6,080/5,820/5,980/6,000/6,000），批 finalize 递推锚逐批校验；深化正文合计 119,558 字符（python 逐行实测，逐行 ≥350 全行字符硬闸 15/15）；④台账回填——总纲 §7.3-D1 B01–B15 十五行 [骨架]→[已深化] 并回填实测字数、D1 小结修订记录追加；⑤四项齐备（定位/边界/判据/正文）。\n"
"- **冻结登记（cross_domain_freeze 一项）**：D1 NT API 语义面三冻结（PEB/TEB 偏移表与版本分档协议 F12003/F12019 族、NTSTATUS 编码面 F12021 族、OBJECT_ATTRIBUTES 协议 F12081 族）→D2/D3/J1（总纲 §7.3-D1 小结 Top10 接口⑥载体落冻）。\n"
"- **open_risks**：R-D1-001（P2 版本锚 ADR-UNX-008 波 07 前裁决，共性字段先行 F12019 兜底）/ R-D1-005（P1 D3 桩期依赖，桩表显式记账真联测切换）/ R-D1-008（P2 真机判据悬置随闸门补测）入 handoff；R-D1-002/006/007/009–014 批级风险已在各批 finalize 表登记。\n"
"- **双同步**：docs 落盘 + git 提交推送（unxreal(d1): B01–B15 finalize 300条（AI-16 域 UNX-D1 NT API 语义面深化收口，行数守恒 86,740/240,000））。共享协调文件（根台账/总纲/handoff）按统一台账 append-only 性质提交并登记背景；他会话产物均不纳入，不越权代提交。\n\n"
)
anchor4 = "## 五、协作纪律速览（新 AI 会话必读）"
if anchor4 in text:
    text = text.replace(anchor4, sess + anchor4, 1); n += 1
else:
    print("MISS sec4")

rev = "| 波08-M06 | AI-16 | D1 域 B01–B15 全批深化收口 300 条/86,740 行（六要素深化册 15 件，逐行正文 ≥350 全行字符硬闸 15/15 实测、深化正文合计 119,558 字符、全域最低 350 恰压线 B07；finalize 五步断言链逐批全过：300 判据 ID 唯一、kernel 三源码目录 F12xxx 零引用 grep 实测 0 命中、行数守恒 86,740 递推锚逐批校验）；总纲 §7.3-D1 十五回填 [已深化]+实测字数+小结修订记录、handoff 增 D1 块/deepen_books 15 册/R-D1 三项主风险账/NT API 语义面三冻结（→D2/D3/J1）、根台账 §二/§三/§四/§六 同步 |"
lines = text.split("\n")
done6 = False
for i in range(len(lines) - 1, -1, -1):
    if lines[i].startswith("| 波07-M02（2026-09-29） | AI-17 | D2 域 B01–B15"):
        lines.insert(i + 1, rev); n += 1; done6 = True; break
if not done6:
    print("MISS sec6")
text = "\n".join(lines)

io.open(RP, "wb").write(text.encode("utf-8"))
print("root ledger updated OK, edits=", n)
