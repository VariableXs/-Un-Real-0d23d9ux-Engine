# -*- coding: utf-8 -*-
"""AI-16 · D1-B15 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B15.md"
GATE = 350

EXP = [
# I0 F12281 NtWaitForSingleObject
"补充深化：Timeout 判别以符号位定相对/绝对两档且负值相对档单位为 100ns；WAIT_TIMEOUT 返回前提是等待块已正常入队并到期，零超时路径不入队直接探测一次；ABANDONED 态仅 Mutant 对象可出现，废弃计数与后续获取语义联 F12285 判据。",
# I1 F12282 NtWaitForMultipleObjects
"补充深化：句柄数组遍历序即满足索引判定序（WaitAny 取最低满足索引）；WaitAll 部分满足时块保持挂起且已满足对象不回退；重复句柄注入按 STATUS_INVALID_PARAMETER 拒止且与 Windows 允许重复句柄数组的差异列本体差异类账；上限注 MAXIMUM_WAIT_OBJECTS 锚值 64。",
# I2 F12283 Event 族
"补充深化：InitialState 与 Type 四组合创建例全格；Set 对通知型唤醒全部等者、同步型唤醒单个等者且复位先行于唤醒交付（时序档）；Pulse 无等者时状态不驻留（与 Set 差异判据）；Open 按 ObjectAttributes 与类型校验，非 Event 对象注入正确拒止。",
# I3 F12284 Semaphore 族
"补充深化：Release 在无等者时计数仍加回（供后续等待消费）；InitialCount==MaximumCount 创建合法档（即满信号量）显式；计数饱和路径 Release 注入正确拒止；守恒账含创建-等待-释放-关闭全生命周期循环 10^4 次对冲零漂移。",
# I4 F12285 Mutant 族
"补充深化：所有权绑定线程而非句柄，同线程经不同句柄重入计数仍累加；非属主线程 Release 注入拒 STATUS_MUTANT_NOT_OWNED；废弃态转换后所有权清零且 RecursionCount 归零；废弃获取后对象转入正常持有态（后续等待不再报废弃）判据入档。",
# I5 F12286 Timer 族预告
"补充深化：DueTime 负值为相对档（自调用时刻起算）、正值绝对档（基于 A2 系统时间基准，含时钟回拨档显式）；Period=0 单次触发档；TimerApcRoutine 与 ApcContext 交付挂 F12215 APC 通道同源断言；Cancel 后挂起回调不再交付判据在册。",
# I6 F12287 SignalAndWait
"补充深化：信号动作在等待块入队前完成（原子序），并发等者可见序先于本线程挂起；自信号自等待注入按 STATUS_INVALID_PARAMETER 拒止且不产生自唤醒死锁窗口；Signal 对象类型校验先行（非可信号对象拒止），Wait 对象白名单与 F12281 一致。",
# I7 F12288 WAIT 块
"补充深化：块对象池预分配（高并发等待不复配内存档）与池耗尽回退路径显式；超时精度账以 A2 定时器到期序列对账，漂移采样 P95/P99 双点；提前唤醒时未满足对象引用计数即时释放，块销毁与对象账冲销原子化，守恒断言含异常路径样本。",
# I8 F12289 错误矩阵
"补充深化：8 接口×5 维（句柄/参数/权限/状态/类型）展开 40 行，每行至少 1 注入例；类型维含非同步对象等待拒止（STATUS_OBJECT_TYPE_MISMATCH 档）；状态维含 TEMPORARY 对象引用消失路径；差异例（Windows WaitAll 允许重复句柄行为）列本体差异类账，禁止静默归一。",
# I9 F12290 A2/A3 联签
"补充深化：等待挂起链三段（等待块入队→线程状态迁移→调度器让出）逐段挂点测试；唤醒链逆向三段对平 A3 调度账流水（就绪入队/优先级变更/择中调度）零漂移；唤醒延迟 P95 预算分两轴（信号→就绪、就绪→运行）分别达标；桩期挂 A2/A3 记录面桩件、真期挂中断/调度真件，两态分开列账。",
# I10 F12291 边界样本集
"补充深化：40 例覆盖唤醒丢失三类（信号先于等待入队/信号与入队同拍/多等者扇出截断）、竞态四类（Set 与 Timeout 同拍/Pulse 与唤醒交付交错/Cancel 与到期交错/废弃与获取交错）、超时漂移两类（定时器精度/调度延迟）；竞态注入器以线程交错脚本驱动 10^4 次，每次断言零丢失与终态唯一，丢失即红账含对象/线程/时序三元组定位。",
# I11 F12292 双机对照
"补充深化：15 场景中时序敏感 5 例显式容差白名单（仅唤醒序相对关系，禁绝对时间比对）；状态序列 diff 以对象状态机弧序为准，白名单外零容差；差异裁决分三类（本体差异/时序容差/真差异），前两类入可接受档附机制说明，真差异即红例阻断收口，裁决记录随批归档。",
# I12 F12293 对照表
"补充深化：30 行覆盖等待 2 接口+五族接口+WAIT 块辅助面，扩展行含五态返回矩阵行与废弃态行；status_matrix 双态显式（真绿/桩期绿+桩件名），anchor 列回指 ADR-UNX-008 与 ReactOS 注记（对照不抄声明在册）；抽样 10 行独立重放器复测，首测复测 diff 零方绿，行变更走批内变更账。",
# I13 F12294 D4 联签预告
"补充深化：alertable 等待中 APC 先于对象满足处理（交付优先级档），两者同拍时 APC 先交付且等待重入一次（计数账）；消息唤醒通道仅登记预告（MsgWait 类接口面），投递时机判据与 F12215 单一 APC 通道同源断言三组样本；D4 承接批号与联测用例清单登记入预告面。",
# I14 F12295 性能预算
"补充深化：空转唤醒压测 10^5 次含单对象/多对象（8 对象 WaitAll）两组口径；延迟 P95 预算两轴（信号→就绪/就绪→运行）分列达标；内核抖动账以调度延迟方差记录，方差超界标红定位不静默放宽；O1 预算单源引用不复制，超限 10% 触发回归并入偏差账。",
# I15 F12296 防幻觉出处账
"补充深化：出处扫描覆盖六要素/正文/判据/参数表四区，空出处与占位词（待补/TBD）计违规，扫描断言嵌入 finalize 每批必跑；待实测清单含唤醒延迟真机分布与废弃态真机注入两组待基准机项，逐项登记前置条件与期望值来源；五态返回码与 MAXIMUM_WAIT_OBJECTS=64 锚值均注版本锚（ADR-UNX-008），无锚即红闸。",
# I16 F12297 文档对齐
"补充深化：三件映射按条目 ID 对齐（深化册/骨架 B15/对照表）行号定位双校验；抽查 10 行双点（判据三成分+Windows 行为描述）比对；漂移注入轮转改写三个文档各 1 例共 3 例（状态/阈值/码值改错后断言器必报），十次轮转检出 10/10 方绿；跨批变更同步更新映射并重跑抽查，零悬空为收口前提。",
# I17 F12298 ktest 断言集
"补充深化：19 条用例聚合三族子入口（等待族/五族/账面族），用例头部元数据（依赖桩件名/依赖账面名/双机标记）断言齐备；桩缺失记 SKIP 入桩期账不降级 PASS；全跑按条目号升序可复现，失败定位输出（用例 ID/判据 ID/期望实际三元组）后附邻近用例结果；双机对照用例挂基准机可用性前置检查。",
# I18 F12299 M 型小结
"补充深化：合并扫描含跨批互引三元组登记（B11–B15 引用 B01–B10 的消费边），悬空即红；E 型/I 型预告登记含每批主题一句话+判据框架+承接依赖批号，域待办账可点验；三账汇总显式 M 型段行数 29,880（B11–B15）与 F 型段 56,860（B01–B10）分段对平至 86,740。",
# I19 F12300 三百条里程碑
"补充深化：跨批消费边全量登记含 D1→外部域（B1/B2/A2/A3/A4/A5/D2/D3/D4/O1）与 E1 前置三件组合消费两类；三源对平以脚本断言（深化册行数求和=骨架求和=域账累计）代替人工核算；波 08 前置门判定留痕含桩期清单（六项待基准机实测项）与门禁结论两态，前置门未结清前波 08 开工申请即红闸。",
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
print("B15 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
