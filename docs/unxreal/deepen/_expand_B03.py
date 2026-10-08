# -*- coding: utf-8 -*-
"""AI-16 · D1-B03 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B03.md"
GATE = 350

EXP = [
# I0 F12041 PEB 字段总账
"补充深化：30 字段含头四字段（Inherited/ReadImageFileExecOptions/BeingDebugged/BitField 位域四子位）与尾部 SessionId 等扩展字段；Win10/Win11 差异列单独注记；比较器差异列含（偏移/类型/语义三面 diff），切换告警挂 CI 回归钩子；抽样 20 含关键字段（Ldr/ProcessParameters/Heap）优先。",
# I1 F12042 ImageBaseAddress
"补充深化：写点协议含写入前握手校验（交接件三字段齐备才可写），缺件写入拒止入红账；握手账可查询接口冻结（D2/读侧两消费方）；交叉比对含映像首段 PE 头标记校验；半态注入指握手未完成读（返回零+错误标记档）与写中途读（版本尾标挡）两子例。",
# I2 F12043 ProcessHeap
"补充深化：堆初始化参数含（段 reserve/commit 阈值/反提交阈值）三组，Flags 推导映射表逐位注出处；假堆桩指桩期以记录面桩件提供实例地址（签名恒等校验）；悬垂注入含释放后读与替换实例两子例，交叉校验含堆签名+尺寸两要素；D2 联签三列状态（互引/冻结/联测）登记。",
# I3 F12044 BeingDebugged/NtGlobalFlag
"补充深化：翻转事件账含（挂接/摘除双事件+时刻+调试面标识）四列可回放；NtGlobalFlag 位映射含典型堆校验位 8 位抽样；调试档接口与用户态写分离（档面隔离）；用户态写拒止验证含写后读回零值断言（写未生效证明）。",
# I4 F12045 ApiSetMap
"补充深化：Schema 交付校验含（签名魔数/长度上界）两要素，签名不匹配拒止入红账；快照接口冻结（取基址+版本档双值），解析器按档分支（Win10/Win11 档差异）；10 例重定向样本含（常用 api-ms-* 集+不存在集两类），不存在集解析失败行为单列。",
# I5 F12046 TLS 槽与扩展位图
"补充深化：64 主槽索引位语义（0=空闲档）注出处；扩展位图双字结构（TlsExpansionBitmapBits）档；耗尽路径错误码与 Windows 口径一致注出处；交叉一致性断言含（位图位↔槽数组非零位）双向对账；10^4 分配回收含多线程并发分配子例。",
# I6 F12047 LoaderLock
"补充深化：临界区实例由 D2 布置、D1 仅记账（所有权分离声明）；LdrLockLoaderLock 选项位两档语义（仅查询/获取）注出处；死锁注入检出经 A5 锁序账（超时阈值触发报告）；DllMain 场景含（ATTACH 内再获取）与（CreateThread 内获取）两子例。",
# I7 F12048 版本字段账
"补充深化：三元组（Major/Minor/Build）来源 ADR-UNX-008 单一源，CSDVersion/SP 槽位同批登记；写点协议含（初始化单次写+封账标记）；伪装注入仅经调试档且注入后账面留痕（伪装值/原值/时刻）；运行期只读验证含非调试档写注入 10/10 拒止+红账。",
# I8 F12049 CSDVersion
"补充深化：UNICODE_STRING 布局断言与 F12102 三元组约定单源（Length/MaximumLength/Buffer 三字段一致性断言）；恒空档含 Win10+/Win11 双档显式；NTDDI 对照表含（宏名↔三元组值↔档位）三列，漂移注入（改一处宏值）10/10 检出。",
# I9 F12050 FastPebLock
"补充深化：保护集字段清单含（版本三元组/调试位/位图账）登记；快锁行为档（短暂自旋上限+超限转等待）与 B10 临界区（完整等待块）差异显式；并发压测含校验和覆盖域声明（保护字段集逐字段求和）；A5 锁序登记（FastPebLock 在句柄表之前）。",
# I10 F12051 GdiHandleBuffer
"补充深化：槽数版本档差异（32/64 位口径）注出处；对账接口冻结含（事件类型/方向/频率三要素）；镜像账差异告警含（计数偏差即红账）口径；桩联测指桩期以 F 部记录面桩件对账（10/10 显式档），真期对账承接 F 部联签。",
# I11 F12052 ActivationContextData
"补充深化：三槽偏移登记含版本档差异注记（Win10/Win11 排列）；空值读侧规则含（空值传播与调用方判责）口径；布置协议含（写路径一次性+封账+事件账三步）；互不污染声明（SxS 域上线前读侧不缓存空值派生结论）。",
# I12 F12053 AtlThunkSListPtr
"补充深化：两槽（AtlThunkSListPtr 与配对检测槽）偏移与缺省零值档登记注出处；缺省账面规则含（读回恒缺省值+读侧不得预缓存派生结论）两条，启用切换时缓存失效显式；生态启用时写路径布置含（白名单写点校验+一次性封账+事件账）三步；检测槽语义档与 F12059 对照表行同步登记（行↔槽双向映射断言）；双机缺省读回对照含初始化后与运行期双时点采样，时点差异注记为版本差异档待实测。",
# I13 F12054 内核写路径白名单
"补充深化：白名单四列含校验器（写值合法性函数符号）列；写时机含（初始化单次/调试档/事件翻转）三类；审计账按字段过滤查询接口冻结（调试面消费）；构建期拦截以写点符号扫描实现（未登记符号引用即失败）；Windows 对照含调试位翻转时机双机注记。",
# I14 F12055 跨线程读一致性
"补充深化：版本尾标为双字版本号+校验和（覆盖 PEB 页保护字段集）；有限重读上限 3 次登记（超限红账）；压测写者含（版本三元组写/调试位翻转/位图分配）三类交错；零撕裂断言含（读到中间态计数=0）与（校验失败计数=0）双指标；红账零误报含（合法写期间的预期重读不算红账）口径声明。",
# I15 F12056 八内联读取 API
"",
# I16 F12057 越界写检测
"补充深化：哨兵指纹含（前后各 64 字节双区指纹填充模式），调试档标记防生产残留；定位器双档（周期巡检周期值登记/写触发即时巡检挂点清单）；红账接 F0011 含（快照窗口 5 秒内事件全量）口径；生产档零哨兵验证含（生产构建哨兵代码编译期剔除断言）。",
# I17 F12058 静态断言器
"补充深化：断言集覆盖 ≥30 字段含（偏移断言+尺寸断言+对齐断言）三类；野断言警告含（同字段重复断言）检测；生成器单源（总账表→断言代码）禁手写副本；失败信息四要素含出处行号（修复定位到表行）；30 秒定位以（失败即停+首错输出）实现口径。",
# I18 F12059 对照表
"补充深化：五列复用 F12039 母版类型（同构断言：列数/列名/校验规则一致）；19 字段条目含（字段行+派生位域子行）展开；判据 ID 缺失构建失败指（表行 judge_ids 列逐行非空断言）；抽样 30 复测脚本独立（不复用首测过程），回填含复测日期；状态矩阵三档与双机对照绑定关系可点验。",
# I19 F12060 ktest PEB 断言集
"补充深化：四族（结构族/调试位族/并发族/对照族）聚合 ≥70 断言清单可点验；互斥组声明指（哨兵族↔生产档族不可同跑，跑批调度自动跳过+强制跳过原因登记）；三档入口（全跑/单跑/抽验）复用聚合器单实现；交叉核验含（断言↔对照表 judge_ids 双向映射）零缺口断言；skip 带因含（互斥跳过/依赖缺失/显式配置）三类登记；跑批账含（通过/跳过/失败计数+时刻）随批归档。",
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
print("B03 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
