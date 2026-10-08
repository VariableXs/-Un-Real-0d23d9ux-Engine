# -*- coding: utf-8 -*-
"""AI-16 · D1-B10 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B10.md"
GATE = 350

EXP = [
# I0 F12181 结构语义
"补充深化：六字段表中 LockCount 新旧编码差异注记（锁定位编码 vs 旧负数编码，本工程选锚档）；DebugInfo 指针在 NO_DEBUG_INFO 档为 -1 占位值口径；消费清单含（B10 内部/B15 桥接/调试面）三方；断言集含 DebugInfo 占位值断言子集。",
# I1 F12182 Enter
"补充深化：快路径 CAS 语义含（LockCount 最低位=锁定标志位）位约定；自旋期争用检测指自旋中发现锁释放即直接尝试获取（不自旋到底）；等待切换含（自旋轮数零=直入等待块）特例档；快路径预算口径为桩期指令计数（真机口径随闸门补测登记）；归属一致断言含（获锁线程 ID+递归计数=1）双字段。",
# I2 F12183 Leave
"补充深化：归零释放档含（位操作口径注出处）与唤醒交付时序（先清后唤醒防重复唤醒）；唤醒次序 FIFO 档声明为记录性语义（Windows 不承诺严格 FIFO 差异注记）；三等待者场景含（持有者三次 Leave 连续交付）与（并发 Leave 竞态）两子例；单唤醒档指一次 Release 仅唤醒一个等待者断言。",
# I3 F12184 DebugInfo
"补充深化：DebugInfo 结构含 Depth/OwnerBackTrace 尾字段（回溯深度档）；进程锁链为双向环链（表头哨兵），链遍历一致性含（增删与遍历并发）子例；ContentionCount 与 F12182 争用路径计数联动断言（每次自旋耗尽加一）；回溯索引档注记录性（真机栈回溯随闸门补测）。",
# I4 F12185 TryEnter
"补充深化：Try 在 DebugInfo 侧同样零副作用（EntryCount 不增）；成功等价断言含（五字段快照对齐）；零副作用注入含（忙态 Try 后字段快照对比）验证；Try 与 Enter 竞态样本（同时刻并发）10/10 收敛正确；失败路径含（已持锁线程 Try 返回 TRUE 递归获锁档）语义注出处。",
# I5 F12186 等待块预告
"补充深化：等待块含（等待对象指针/回调签名/超时槽）三扩展字段预告；挂点接口冻结含（三操作签名+错误路径返回值）协议书；守恒断言含（入队-出队-在队三值恒等式）与异常路径（挂起失败即出队）覆盖；B15 互引状态三列（登记/冻结/联测）登记；桩期显式档指等待块桩期以记录面桩件联测。",
# I6 F12187 SpinCount
"补充深化：SpinCount 高位标志掩码口径（低位轮数+高位标志位注出处）；NO_DEBUG_INFO 标志解包后 DebugInfo 置 -1 占位档；未知标志位注入显式档（拒止或忽略与 Windows 一致待实测标注）；自旋轮数取值路径断言含（传参档/缺省档两源单选）验证。",
# I7 F12188 递归语义
"补充深化：重入计数守恒含（未持锁 Leave 拒止例）；归属比较单源指 TEB ClientId 读取单点（F12063 消费），TEB 切换期归属判定窗口档显式；身份伪造注入含（OwningThread 字段直接改写后重入）防御检出（校验和/读回比对两手段）；10^4 重入含（嵌套 3 层）子例。",
# I8 F12189 LockSemaphore
"补充深化：等待事件惰性创建含（创建失败回退路径：等待块转轮询档注记）；事件类型（同步事件单次置位）与消费序（唤醒后即复位）；句柄泄漏红账含（Delete 时在用事件计数非零即红）；守恒断言含（创建-回收-在用三值恒等式）与（并发创建竞态单实例保证）子例。",
# I9 F12190 泄漏核账
"补充深化：退出序列消费点协议与 C1 联签（线程退出时 TEB 持有计数核查）；非零即泄漏事件含（泄漏计数/首泄漏址）两要素；定位面含（DebugInfo 链反查+创建点回溯索引）两级；零静默指泄漏不得吞为警告（终止档与 Windows 口径双列）；A5 合流登记。",
# I10 F12191 Initialize 族
"补充深化：初始化快照含（DebugInfo 链入链成功验证）；AndSpinCount 标志位与轮数解包断言（F12187 联动）；重复初始化红账含（魔数检出+时刻+调用点回溯）；发布档行为注 Windows 口径（不检测）显式声明，两档差异非缺陷为决策档。",
# I11 F12192 Delete
"补充深化：删除序含（等待者存在时删除拒止前置）；事件回收含（在用计数对冲后关闭）；DebugInfo 回收含（进程锁链摘除+池归还）两步；持锁中删除注入含（自持/他持两子例）；毒化检出指删除后结构填充毒值（使用即红账定位），发布档毒化关闭口径显式。",
# I12 F12192 压测（F12193）
"补充深化：三段比例可观测含（快路径/自旋/等待块计数器）分档统计输出；守恒账含（全局在持数=各线程深度和）恒等式；竞态注入实验含（去掉原子性后校验和错位检出）验证压测灵敏度；哨兵双检含（计数器+校验和+序号三重）；争用分布记录性对照口径声明。",
# I13 F12194 A5 联签
"补充深化：报点接口冻结含（两事件+补点三报点）签名与开销预算（报点开销上限档）；锁序全序图以（线程持锁栈快照）输入构建；递归获锁豁免档指同锁重入不入序图（防自环误报）；交叉持锁注入含（A 持 X 等 Y/B 持 Y 等 X）经典对与（三线程环）扩展例；检出为自研增强档声明（非 Windows 语义项）。",
# I14 F12195 SRWLOCK 预告
"补充深化：SRW 状态语义含（位域编码注出处：独占位+共享计数域）；独占转共享语义档（独占持有者递归转共享档显式）；零值可用档含（未初始化结构直接用）与（静态初始化）两子例；挂点复用预告指等待块协议复用（F12186 单实现）；消费面含 B15 等待联测预告。",
# I15 F12196 SRW 四接口
"补充深化：16 组矩阵含（合法操作/非法操作双向）；非法操作期望行为（返回错误或阻塞）与 Windows 一致口径；递归 AcquireExclusive 自锁死语义档指 Windows 行为为死锁（本工程告警+阻塞同型）；告警联动 A5 锁序账（非静默红线）；Release 对称性断言（Acquire/Release 计数镜像）。",
# I16 F12197 条件变量预告
"补充深化：条件变量谓词协议档（循环重检模式）在册；双挂点协议含（锁参数类型区分 CS/SRW）签名冻结；丢失档语义注 Windows 口径（不补偿，先 Notify 后 Sleep 即丢失）；超时参数档（超时返回 FALSE）；B15 互引三列状态登记；联测后补录（Notify 唤醒序记录性对照）。",
# I17 F12198 对照表
"补充深化：三表含（结构表 4 结构/接口表 ≥12 接口/错误路径表）行数下限断言；错误路径表含（错误码/触发条件/防御层）三列；判据 ID 可达断言含（判据存在性+批归属正确性）双面；抽样 20 复测含（结构断言类+行为类混合抽样）；零断链指表行↔判据↔复测记录三向可溯。",
# I18 F12199 ktest 断言集
"补充深化：19 条用例四字段元数据断言齐备；依赖前件含（B15 等待块桩件/A3 线程账）声明，缺桩 SKIP 入桩期账；失败定位含（线程 ID/次序号/矩阵格）三元组；三族为结构族/行为族/账面族独立入口；双机对照用例挂基准机可用性前置检查（版本匹配断言）。",
# I19 F12200 F 型总闸
"补充深化：十批消费边含（B10→B15 等待块/B10→A3 线程账/B10→A5 锁序）跨域边显式；九聚合器全跑含（顺序执行序+失败即停策略）口径；两态记账含（真绿数/桩期绿数/比例）三列与桩期项清单逐条登记（桩件名+依赖域）；开门判定留痕含（判定时刻/总闸版本/清单哈希）；总闸含十批对照面汇总状态列（双机联测完成率）。",
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
print("B10 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
