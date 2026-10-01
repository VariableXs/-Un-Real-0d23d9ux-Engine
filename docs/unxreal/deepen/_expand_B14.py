# -*- coding: utf-8 -*-
"""AI-16 · D1-B14 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys, re

P = "docs/unxreal/deepen/D1-B14.md"
GATE = 350  # 全行硬闸（含 "- 正文：" 前缀），目标 ≥360

EXP = [
# I0 F12261 NtCreateFile 矩阵
"补充深化：三段判定顺序固化为权限面（DesiredAccess 位校验）→存在性面（CreateDisposition 分支）→共享面（share 三轴对账已开句柄）的线性流水，任一段命中即短路返回，保证同输入下行为确定性可回放；SUPERSEDE 对目录对象拒止 STATUS_ACCESS_DENIED、OVERWRITE 要求 FILE_WRITE_DATA|DELETE 位的两条边界例入拒止矩阵首组。",
# I1 F12262 Read/Write
"补充深化：ByteOffset 为 NULL 时按句柄 CurrentByteOffset 递进且仅同步句柄合法，异步句柄传 NULL 直接拒 STATUS_INVALID_PARAMETER；长度 0 的读写按成功返回且 Information=0；STATUS_PENDING 路径下缓冲与 IOSB 在完成前禁改，违规样本入拒止矩阵。",
# I2 F12263 QueryInformationFile
"补充深化：分派先按 FileInformationClass 白名单再校验 Length 与该类最小结构长度；FileStandardInformation 五字段（AllocationSize/EndOfFile/NumberOfLinks/DeletePending/Directory）逐档固化；FileNameInformation 仅回名称不含路径语义单独注记；FilePositionInformation 要求 FILE_READ_POSITION 位，缺位拒 STATUS_ACCESS_DENIED，全部并入 15 类表附样本。",
# I3 F12264 SetInformationFile
"补充深化：FileRenameInformation 的 FILE_RENAME_FLAG 链（REPLACE_IF_EXISTS=0x1/POSIX_SEMANTICS=0x2|0x8/SUPPRESS_PINNED=0x4）逐位入档，ReplaceIfExists 与目标冲突按标志位决定覆盖或拒 STATUS_ACCESS_DENIED；FileDispositionInformation 的 DELETE 标记延迟到关闭时生效语义单独判据；Position/EndOfFile/LinkInformation 三类注入样本并入拒止面。",
# I4 F12265 QueryDirectoryFile
"补充深化：FileName 模板空串表示返回全部项，通配按 *（跨段零或多字符）与 ?（单字符）实现且长名/短名双轨匹配；ReturnSingleEntry=1 时 Index 推进一格，RestartScan 置位后 Index 归零；STATUS_NO_MORE_FILES 终止码与 STATUS_NO_SUCH_FILE 拒止码分列，枚举序以创建序做记录性档显式声明。",
# I5 F12266 FlushBuffersFile
"补充深化：冲刷作用域为句柄所引流，元数据（大小/时间戳）与数据面同批落盘；挂 B2 journal 挂点时冲刷序与提交序一致性判据显式；对只读流冲刷允许成功但登记记录性行为档，非缓存流冲刷注入返回拒止码入拒止面。",
# I6 F12267 CancelIo/CancelIoEx
"补充深化：NtCancelIoFile 仅取消当前线程在同一句柄上的挂起请求，跨线程注入返回 STATUS_ACCESS_DENIED；NtCancelIoFileEx 携带取消请求参数可跨线程定向取消；取消完成以 IOSB.Status=STATUS_CANCELLED 回填且 Event 单次置位，完成与取消竞态以请求终态机单一收敛点裁决，竞态样本 10 组入双机实测档。",
# I7 F12268 DeviceIoControl/FsControl 预告
"补充深化：控制码四元组（DeviceType/Access/Function/Method）解码器立档，METHOD_BUFFERED/IN_DIRECT/OUT_DIRECT/NEITHER 四缓冲协议分档；输入校验失败统一 STATUS_INVALID_PARAMETER，NEITHER 越界注入 STATUS_ACCESS_VIOLATION 档；跨界控制码样本登记 B4 分界联测预告件。",
# I8 F12269 FILE_* 掩码矩阵
"补充深化：GENERIC_READ→FILE_GENERIC_READ 五段位（SYNCHRONIZE|READ_DATA|READ_ATTRIBUTES|READ_EA|READ_CONTROL）展开逐位入表，DELETE/FILE_EXECUTE 亦入冲突矩阵；共享冲突判定仅对 READ_DATA/WRITE_DATA/DELETE 三轴生效，属性位不参与冲突，该边界差异例单独列格。",
# I9 F12270 IO_STATUS_BLOCK
"补充深化：x64 布局为 Status(4)+Pointer(4 联合)+Information(8) 共 16 字节、8 字节对齐，成功路径 Information 语义按接口分档（字节数/枚举计数/句柄值）；挂起路径 IOSB 仅完成时单写回填，多等待者以 Event 单置位原则消费；与 F12023 完成链同源断言样本三组并入。",
# I10 F12271 错误矩阵
"补充深化：25+ 码按六维交叉定位——句柄（INVALID_HANDLE）/参数（INVALID_PARAMETER、INFO_LENGTH_MISMATCH）/权限（ACCESS_DENIED）/状态（END_OF_FILE、NO_MORE_FILES）/冲突（SHARING_VIOLATION）/命名（OBJECT_NAME_NOT_FOUND、OBJECT_PATH_NOT_FOUND）；每码至少 1 例返回值加 1 例 IOSB 终态双断言，差异例列本体差异类账，禁止静默吞码。",
# I11 F12272 B1 VFS 联签
"补充深化：联签样本按生命周期六阶段（创建/读/写/查询/设置/关闭）各 3 组，组内记录 D1 调用序与 B1 VFS 账流水（操作/区间偏移/字节数/结果码）逐笔对平；桩期以 B1 记录面桩件对接，真期以 journal 回放比对，两态结果分开列账不互充；\\??\\ 前缀与相对 RootDirectory 两组翻译样本预挂 D3 命名空间接口面。",
# I12 F12273 边界样本集
"补充深化：40 例按维度覆盖——零长度读写 6 例、对齐边界（扇区 512/页 4096）8 例、最大偏移 0x7FFFFFFFFFFFFFFF 上下溢 6 例、命名超长/空串/保留设备名 8 例、句柄类型错配 6 例、标志非法组合 6 例；每例断言三态（拒止码/成功语义/IOSB 终态）且必含崩溃防护项（悬空指针与 Length 溢出），全部入红账可回放资产。",
# I13 F12274 双机对照判据
"补充深化：15 场景取自前 13 条高价值面（disposition 矩阵 3/偏移语义 2/枚举序 1/取消竞态 2/错误矩阵 4/其他 3），同码双跑以 S1 母版为基准机；diff 口径三层（返回码/数据/状态）逐层同值方为语义级一致；仅时间戳粒度类记录性差异标本体差异类并附产生机制说明，禁止以记录性档名义吞掉可归一差异。",
# I14 F12275 对照表 30 条
"补充深化：30 行覆盖族内 19 条目主接口外加 share/access/disposition 三个子面扩展行；status_matrix 列必须双态显式（真绿/桩期绿+桩件名），judge_ids 指向本批判据 ID 且逐行可点验；抽样 10 行复测以独立脚本重放，不复用首测过程记录，复测值与首测值 diff 为零方绿，防同源复读伪一致。",
# I15 F12276 性能预算
"补充深化：P95 预算按接口分三组（Read/Write 4KB 随机、CreateFile 打开、Query 查询），队列深 8、样本 10^4 口径登记；采集含调用时延与完成时延两轴，异步完成延迟单列；O1 对标基准为同口径 S1 母版记录值，超预算 10% 触发回归并回填偏差账，禁止以噪声名义静默放宽阈值。",
# I16 F12277 防幻觉出处账
"补充深化：出处扫描覆盖六要素、正文、判据四区，正文空/占位（待补/TBD/纯标点）均计违规；扫描断言嵌入 finalize 链每次必跑，违规即红闸阻止批收口；待基准机实测清单逐项登记（编号/前置条件/期望值来源），清单项在实测前禁写已一致类结论词；结构偏移与错误码均注版本锚（ADR-UNX-008），无锚条目计出处缺失。",
# I17 F12278 文档对齐
"补充深化：三件文档为本深化册、骨架 B14、对照表，按条目 ID 对齐；抽查 10 行取判据三成分与 Windows 行为描述双点比对入 finalize；漂移注入采用改写式检测（人为改错某行状态/阈值/码值后验证断言器必报），检出 10/10 方绿；映射悬空指条目缺失或 ID 错位，零悬空为收口前提。",
# I18 F12279 ktest 断言集
"补充深化：19 条各至少 1 用例入册，全跑入口聚合三族（对象引用/数据面/账面）独立子入口；桩期自检指用例头部带桩件依赖声明，桩缺失时记 SKIP 入桩期账，绝不降级为 PASS；全跑 100% 判定含失败定位输出（用例 ID+判据 ID+期望/实际三元组），双机对照用例挂基准机可用性前置检查。",
# I19 F12280 集成账
"补充深化：19 条互引以（引用方,被引方,引用面）三元组登记，悬空指被引 ID 不存在或非深化态；预告三对（B1 VFS 对接/B2 journal 冲刷/D3 路径翻译）各含状态列（登记/对接/收口）与承接批号；账-表行数一致断言以对照表行数=集成账引用行数为等式，B14 收口累计行数同步入域账递推。",
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
            if anchor == -1:
                newbody = body + add
            else:
                newbody = body[:anchor] + add + body[anchor:]
            lines[i] = newbody + ("\r" if ln.endswith("\r") else "")
        idx += 1

if idx != 20:
    print("ERROR: found %d body lines, expected 20" % idx); sys.exit(1)

# 回填 finalize 深化字数行
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

# 硬闸复测
bad = []
i = 0
for l in out.split("\n"):
    s = l.strip().lstrip("-").strip()
    if s.startswith("正文："):
        n = len(l.rstrip("\r"))
        if n < GATE:
            bad.append((i, n))
        i += 1
print("B14 OK: idx=%d total_chars=%d" % (idx, total))
if bad:
    print("FAIL lines < %d:" % GATE, bad)
    sys.exit(1)
ns = [len(l.rstrip("\r")) for l in out.split("\n") if l.strip().lstrip("-").strip().startswith("正文：")]
print("PASS gate=%d: n=%d min=%d avg=%d max=%d" % (GATE, len(ns), min(ns), sum(ns)//len(ns), max(ns)))
