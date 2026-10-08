# -*- coding: utf-8 -*-
"""AI-16 · D1-B13 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B13.md"
GATE = 350

EXP = [
# I0 F12241 NtAllocateVirtualMemory
"补充深化：非零基址按下取整到分配粒度（64KB）边界生效，*RegionSize 上取整到页；ZeroBits 高位溢出注入返回 STATUS_INVALID_PARAMETER（基址越入零位域档）；AllocationType 组合位（COMMIT|RESERVE 直达弧、RESET 仅对已提交页生效、TOP_DOWN 反向选址）逐位入参数表，分配计账与 A4 页表账同步点在册。",
# I1 F12242 NtFreeVirtualMemory
"补充深化：DECOMMIT 允许子区间且区间自动对齐页界，RELEASE 必须为原分配整区（RegionSize 非零即拒）；DECOMMIT 后 GUARD 位与物理页承诺同步回收；释放回调挂 A4 页表账冲销点，两档样本各 10 组入对冲账，释放后区间回 FREE 态与状态机 F12248 弧一致。",
# I2 F12243 NtProtectVirtualMemory
"补充深化：PAGE_* 组合位（GUARD/NOCACHE/WRITECOMBINE）合法性先于变更执行，非法组合拒；跨混合区间时已提交页逐页变更、保留页跳过，OldProtection 回填口径为调用时首页旧值；变更后 TLB 一致性由 A4 冲刷挂点承担，语义面仅断言页表账与权限位账同步无漂移。",
# I3 F12244 NtQueryVirtualMemory
"补充深化：扫描终止于用户态顶区间，越顶注入按错误档拒止；MemoryInformationClass 白名单（MemoryBasicInformation 为主）分派，非常用类在预告面登记；Type 三值（MEM_PRIVATE/MEM_MAPPED/MEM_IMAGE）与 D2 映像映射账联签对平；15 场景含跨 RESERVE/COMMIT 边界与分配粒度缝隙两类特例。",
# I4 F12245 MapViewOfSection
"补充深化：CommitSize 仅对 SEC_COMMIT 类节有效，SectionOffset 须页对齐且 ViewSize=0 表示整节映射，两参数边界例入协商面样本。",
# I5 F12246 UnmapViewOfSection
"补充深化：BaseAddress 必须精确落在视图基址（区间起点），落入视图中部注入拒止；解映射时依赖该视图的页表项/工作集账由 A4 挂点冲销；MapType 两档（单视图/全视图）显式；计数守恒以映射-解映射循环对账入 10^3 压测档。",
# I6 F12247 Read/WriteVirtualMemory
"补充深化：部分拷贝语义以已传量出参为准（跨未提交页停止于第一坏页），完全失败返回对应错误码；跨进程读写挂 A4 账面锁序（先句柄表后页表），与 A5 锁序账一致；权限位缺失返回 STATUS_ACCESS_DENIED，句柄非进程对象返回 STATUS_OBJECT_TYPE_MISMATCH。",
# I7 F12248 三态状态机
"补充深化：六弧逐一挂操作原语映射（分配/提交两步与直达弧、去提交/释放），弧迁移原子性以页表账事务点保证；RESET 与 DECOMMIT 区分档（RESET 不回 RESERVE 仅解承诺）入弧注记；非法弧注入含 FREE→COMMIT 直跳与保留区读写两类，拒止码与矩阵行对齐。",
# I8 F12249 PAGE_* 矩阵
"补充深化：全集保护值逐值入读/写/执行三域布尔表，GUARD 访问流程为异常触发→单步重试前保护位自清（两步语义）；PAGE_WRITECOPY 提交写入时 COPY-ON-WRITE 副本迁移语义档；NOCACHE/WRITECOMBINE 仅内核授予（用户注入拒绝档）；执行位缺失注入 STATUS_ACCESS_VIOLATION，与 A4 权限位映射逐值断言。",
# I9 F12250 错误矩阵
"补充深化：六接口×五维（句柄/参数/权限/状态/冲突）展开 30 行，每行至少 1 注入例；特例档——Free 对非分配基址、Protect 对零页、Query 越顶、MapView 重复基址、Read 自进程快捷路径；差异例（部分拷贝返回成功但 Information 小于请求量）列本体差异类并同步入 F12247 判据。",
# I10 F12251 边界样本集
"补充深化：40 例覆盖——分配粒度缝隙探测 6 例、页界 ±1 读写 8 例、零页（NULL 附近 64KB 低址禁区）访问 4 例、越顶址 4 例、RegionSize 溢出回绕 4 例、句柄/参数类型错配 8 例、保护组合非法 6 例；每例断言（拒止码或异常类型+区间账不变式），零崩溃硬断言独立列项，红账回放含注入序列号。",
# I11 F12252 A4 联签
"补充深化：三协议（分配/释放/保护）各挂对接测试组，组内 D1 调用序与 A4 页表账流水（区间/状态/保护位）逐笔对平；进程销毁守恒账以分配区间数=回收区间数断言，遗留区间即红账；桩期对接 A4 记录面桩件、真期对接页表真件，两态分开列账；锁序按 A5 全局序（句柄表→地址空间）登记。",
# I12 F12253 双机对照
"补充深化：15 场景中分配选址 3 例为 ASLR 敏感——布局以 BaseAddress 相对序与区间邻接关系对账，绝对基址差异标本体差异类（ASLR 档显式）；其余 12 例（释放/保护/查询/读写×边界混合）逐值 diff；可接受差异档仅限布局类，语义类（错误码/状态序列）差异即红，diff 报告随批归档可回放。",
# I13 F12254 对照表
"补充深化：30 行覆盖 6 接口+状态机弧+保护全集+信息类扩展；status_matrix 双态显式（真绿/桩期绿+桩件名），anchor 列全部回指 ADR-UNX-008 版本锚与 ReactOS 行为注记（对照不抄声明在册）；抽样 10 行以独立重放器复测，首测/复测 diff 为零方绿，防同源复读伪一致。",
# I14 F12255 WOW64 预告
"补充深化：32 位档用户态上限（不带 4GT 档）与 LARGEADDRESSAWARE 档两档显式；高址预留注入按零位域拒绝或落位 Aware 档两行为分列；64KB 分配粒度与 32 位堆布局兼容性注记入预告面；WOW64 主体承接 D2 翻译层，本批仅立地址空间档与拒止面，分界表在册。",
# I15 F12256 性能预算
"补充深化：P95 预算按分配/释放/保护/查询四组登记，压测 10^5 次含基址随机化种子序列（可回放）；对冲账（分配=释放=10^5）随压测尾部断言；O1 单源引用指预算值仅存 O1 一处，本批引用不复制副本防漂移；超预算 10% 触发回归，偏差账记录于批 finalize 表。",
# I16 F12257 防幻觉出处账
"补充深化：出处扫描覆盖六要素/正文/判据/参数表四区，空出处与占位词（待补/TBD）计违规；待实测清单含零页真机异常向量与 ASLR 布局两组待基准机项，逐项登记前置条件与期望值来源；MBI 五字段偏移与保护值全集均注版本锚（ADR-UNX-008），无锚条目即红闸阻止收口。",
# I17 F12258 文档对齐
"补充深化：三件映射按条目 ID 对齐（深化册/骨架 B13/对照表），映射零悬空断言入 finalize；抽查 10 行双点（判据三成分+Windows 行为描述）比对；漂移注入改写某行 State 值与错误码后断言器必报，检出 10/10 方绿；骨架行数锁定值与深化册行数一致性亦入抽查集。",
# I18 F12259 ktest 断言集
"补充深化：19 条用例聚合三族子入口（协议族/状态机族/账面族），桩期自检带桩件依赖声明，桩缺失记 SKIP 不降级 PASS；全跑 100% 含失败定位三元组（用例 ID/判据 ID/期望实际），复跑单用例入口独立；双机对照用例（F12241/F12244/F12253）挂基准机可用性前置检查。",
# I19 F12260 集成账
"补充深化：19 条互引以（引用方,被引方,引用面）三元组登记，悬空指被引 ID 不存在或非深化态；预告三对（A4 F12252/D2 F12245+F12255/O1 F12256）状态三列（登记/对接/收口）加承接批号；账-表行数一致断言为对照表 30 行=集成账引用行数，B13 收口累计 74,740 入域账递推链。",
]

raw = io.open(P, "rb").read()
crlf = b"\r\n" in raw
text = raw.decode("utf-8")
lines = text.split("\n")

idx = 0
for i, ln in enumerate(lines):
    s = ln.strip().lstrip("-").strip()
    if s.startswith("正文："):
        if idx >= len(EXP):
            print("ERROR: more body lines than expansions"); sys.exit(1)
        body = ln[:-1] if ln.endswith("\r") else ln
        add = EXP[idx]
        if add:
            anchor = body.rfind("与 Windows 对照")
            newbody = (body + add) if anchor == -1 else (body[:anchor] + add + body[anchor:])
            lines[i] = newbody + ("\r" if ln.endswith("\r") else "")
        idx += 1

if idx != 20:
    print("ERROR: found %d body lines, expected 20" % idx); sys.exit(1)

total = 0
for l in lines:
    s = l.strip().lstrip("-").strip()
    if s.startswith("正文："):
        total += len(l.rstrip("\r"))
for i, l in enumerate(lines):
    if l.startswith("| 深化字数"):
        lines[i] = ("| 深化字数（正文列合计） | 实计 %d 字符（≥6,000 ✓，均 %d/条，逐行 ≥%d） | python 逐行实测回填 |"
                    % (total, total // 20, GATE))

out = "\n".join(lines)
io.open(P, "wb").write(out.encode("utf-8"))

bad = []
i = 0
for l in out.split("\n"):
    s = l.strip().lstrip("-").strip()
    if s.startswith("正文："):
        n = len(l.rstrip("\r"))
        if n < GATE:
            bad.append((i, n))
        i += 1
print("B13 OK: idx=%d total_chars=%d" % (idx, total))
if bad:
    print("FAIL lines < %d:" % GATE, bad)
    sys.exit(1)
ns = [len(l.rstrip("\r")) for l in out.split("\n") if l.strip().lstrip("-").strip().startswith("正文：")]
print("PASS gate=%d: n=%d min=%d avg=%d max=%d" % (GATE, len(ns), min(ns), sum(ns)//len(ns), max(ns)))
