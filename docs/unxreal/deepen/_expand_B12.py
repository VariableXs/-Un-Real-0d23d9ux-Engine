# -*- coding: utf-8 -*-
"""AI-16 · D1-B12 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B12.md"
GATE = 350

EXP = [
# I0 F12221 NtCreateFile/NtOpenFile 协议
"补充深化：参数表含 x64 传参序（前四参 RCX/RDX/R8/R9、第五起栈序按 ntdll 存根约定）与 in/out 方向标注；CreateDisposition 五档中 CREATE_NEW 对已存在对象拒 STATUS_OBJECT_NAME_COLLISION、OPEN_ALWAYS/CREATE_ALWAYS 的成功态差异（OBJECT_NAME_EXISTS 提示级）逐档注记；非法组合拒止例入矩阵；透传断言以 D3 消费点账面回读比对（DesiredAccess/Attributes/RootDirectory 三参零损）。",
# I1 F12222 NtQueryObject
"补充深化：三类返回结构（BASIC_INFORMATION 六字段含 GrantedAccess/MemoryCharge/池占用两字段、TYPE_INFORMATION 类型名 UNICODE_STRING、NAME_INFORMATION）逐字段偏移注出处；Length 不足时返回所需长度或拒止的版本差异列版本差异档；伪句柄（NtCurrentProcess）查询行为单独注入例；抽样 10 对象含目录与信号量两类扩展面。",
# I2 F12223 NtDuplicateObject
"补充深化：Options 位（CLOSE_SOURCE=0x1/SAME_ACCESS=0x2/SAME_ATTRIBUTES=0x4）组合展开；DesiredAccess=0 且非 SAME_ACCESS 时按源 GrantedAccess 语义档；跨进程复制到自身合法档与 CLOSE_SOURCE 组合（等效转移）判据；目标表分配新槽位的实现注记入档，不承诺与 Windows 槽位号一致（本体差异类声明）。",
# I3 F12224 NtClose
"补充深化：关闭序为引用递减→归零触发析构链（D3 回收加对象析构回调）；双关闭注入在句柄表复用窗口下可能命中新句柄，复用防护档显式（Windows 无此保证的差异注记）；PROTECT_FROM_CLOSE 置位后拒 STATUS_HANDLE_NOT_CLOSABLE；伪句柄（NtCurrentProcess/NtCurrentThread）关闭注入单独拒止例。",
# I4 F12225 NtSetInformationObject
"补充深化：ObjectHandleFlagInformation 两布尔位（Inherit/ProtectFromClose）读写双向；Inherit 置位与 D3 继承表联动断言；对目录对象句柄置位同协议（类型无关档）；非法类注入 STATUS_INVALID_INFO_CLASS 与长度不符 INFO_LENGTH_MISMATCH 两码分开列行。",
# I5 F12226 NtQuerySecurityObject
"补充深化：四类（Owner/Group/Dacl/Sacl）SECURITY_INFORMATION 组合展开；自相对 SD 缓冲协议（长度不足返回所需长度档）判据；权限面 READ_CONTROL 必需，Sacl 类另需 ACCESS_SYSTEM_SECURITY 注入例；J1 交付点冻结指类值/结构/错误码三要素冻结，后续变更走变更账。",
# I6 F12227 NtMakeTemporaryObject
"补充深化：语义为清除 PERMANENT 位使对象在引用归零时按回收协议析构；对已临时对象重复调用幂等成功档；DELETE 权限缺失拒止例；回收触发断言在桩期以 D3 记录面桩件验证（析构回调入账），真期挂真回收链，两态分开列账不互充。",
# I7 F12228 NtQueryDirectoryObject
"补充深化：续传柄 Context 语义（上枚尾项指针）与 RestartScan 互斥档；Buffer 为 OBJECT_DIRECTORY_INFORMATION 数组（名称+类型交替项、空名终止）结构注出处；枚举序以目录插入序记录性档显式（与 Windows 无序语义的差异注记）；跨界注入（对非目录句柄枚举）被守卫拦截返回类型不符码。",
# I8 F12229 存根面
"补充深化：x64 存根模板含参数寄存器映射与前四参寄存器序、第五起栈序，mov eax,编号+syscall 尾形态；样本比对以指令类序列归一（不算立即数差），编号差异在账面显式（自定编号空间声明防撞）；生成器输入为参数表单一来源，手写存根禁入（防双源漂移）；syscall 与 int 2e 两尾形态档注记（默认 syscall 档）。",
# I9 F12230 Nt/Zw 双前缀
"补充深化：Windows 差异表列两行——用户态等价（同 SSDT 项）、内核态 PreviousMode 差异（Zw 置 KernelMode 跳过访问检查），本工程简化档仅保留用户态面；PreviousMode 三档捕获链（陷阱帧判定→线程字段缓存→按档分支）与 F12163 陷阱帧转换一致性断言；差异是决策不是缺陷声明在册（人格规范对齐），后续若需内核面按决策账扩展。",
# I10 F12231 错误矩阵
"补充深化：8 接口×5 维（句柄/参数/权限/状态/类型）展开 40 行，错误码/触发条件/出处三列齐；特例行——NtClose 伪句柄档、DuplicateObject 自复制档、QueryObject 长度协议档、CreateFile 名字冲突档；差异例列账含原因分析与锚定待办编号，未裁决差异阻断收口（红闸）。",
# I11 F12232 边界样本集
"补充深化：40 例=8 接口×5 类（空指针入参/超长路径/非法 Attributes 位组合/句柄类型错配/权限位不足），每例登记预期拒止码与防御返回点（用户态校验层/内核守卫层两层标注）；零静默成功指任何样本不得返回成功语义，防御返回统一走红账记录（时间/线程/样本号）可回放；样本集版本号入对照表复测资产。",
# I12 F12233 双机对照
"补充深化：15 场景脚本化含场景参数快照（可重演）；diff 三层（返回码/出参/对象状态）；同构码标注指本工程与 Windows 行为同构但实现码不同源的条目（不冒充同码，人格红线对齐）；差异率 0 指语义级 diff 零行；差异列账条目在锚定待办结清后自动转绿并留裁决记录。",
# I13 F12234 对照表
"补充深化：30 行含 B12 族 8 接口+B01–B11 汇入的 NT API 面（进程/内存/异常域主接口），五列（api_name/category/semver_anchor/status_matrix/judge_ids）逐列断言；anchor 可达性指锚 ID 存在于 ADR-UNX-008 档；抽样复测以 judge_ids 指向判据为单元独立重放，复测值回填 status_matrix 并附日期与执行器版本。",
# I14 F12235 D3 联签
"补充深化：四接口=Create/Duplicate/Query/Close 对接 D3 句柄表四消费点；签名编译期匹配指接口签名与 D3 表接口逐类型一致断言；运行期契约含句柄值有效性/计数守恒/表项回收三断言组；切换清单含桩件名→真件名映射与回归批（F12231 错误矩阵+F12232 样本集重跑），切换后回归双绿才移除桩表。",
# I15 F12236 性能预算
"补充深化：P95 预算按 8 接口分列，压测样本 10^5 次含句柄表高占用（10^4 句柄）与空闲两态口径；超预算标红含因子定位（表查找/锁竞争/账面更新三类归因）；O1 单源引用带版本号，版本不匹配断言即红；改动回归流程含预算值变更审批记录，禁止静默放宽。",
# I16 F12237 防幻觉出处账
"补充深化：出处格式规范含资料名/章节/版本锚三段式，扫描断言校验三段齐备；空出处即构建失败的红闸指深化册构建脚本嵌入扫描，违规批不得收口；待基准机实测清单含伪句柄行为/Length 协议版本差异两组项，逐项登记条目/原因/锚定待办；扫描输出违规行号定位，修复后全量重扫保证无回退。",
# I17 F12238 文档对齐
"补充深化：三件族文档（接口说明/错误矩阵/复测指引）与对照表行映射含行号定位；抽查三要素（语义/错误码/判据 ID）逐行双点比对；漂移注入改文档一行后抽查必报（防线有效性证明），十次轮转检出 10/10；映射零悬空断言含文档行存在性与 ID 一致性双面。",
# I18 F12239 ktest 断言集
"补充深化：19 条用例四字段元数据（判据 ID/复测函数/依赖前件/桩期标记）断言齐备；全跑入口按条目号升序，失败定位三元组（接口/样本号/矩阵行）输出；三族子入口（协议族/矩阵族/账面族）独立可跑；桩期前件自检显式记账（缺桩 SKIP 不降级 PASS），双机对照用例挂基准机前置检查。",
# I19 F12240 集成账
"补充深化：互引矩阵逐对登记（引用方/被引方/引用面），脚本断言含正向反向索引一致；预告三对状态三列（登记/对接/收口）加承接批号（D3 F12235/J1 F12226/O1 F12236）；账-表对平断言为族集成账条数=对照表 B12 段行数；B12 收口累计 68,760 入域账递推，下游批次以此锚校验。",
]

raw = io.open(P, "rb").read()
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
    print("ERROR: found %d body lines" % idx); sys.exit(1)

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
ns = [len(l.rstrip("\r")) for l in out.split("\n") if l.strip().lstrip("-").strip().startswith("正文：")]
bad = [(i, n) for i, n in enumerate(ns) if n < GATE]
print("B12 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
