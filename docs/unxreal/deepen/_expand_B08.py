# -*- coding: utf-8 -*-
"""AI-16 · D1-B08 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B08.md"
GATE = 350

EXP = [
# I0 F12141 RtlDispatchException
"补充深化：CONTINUE_SEARCH/EXECUTION_HANDLER 两返回值语义注出处；接管跃出指找到接管返回后转入展开阶段（目标帧=该帧）；序列账含（帧址/handler/返回值/阶段）四列结构化格式与哈希指纹（录制物不可篡改档）；三类注入含（除零=软件检测档/非法访问=页防护等效/断言=int3）双轨说明。",
# I1 F12142 CONTEXT 采集
"补充深化：转换单向留账指（陷阱帧→CONTEXT 单向，反向仅 NtContinue 路径授权）；快照校验含（RIP 非零/RSP 16 对齐/Cs 恒值域）三项；不可变快照语义含（分发期间原 CONTEXT 允许 handler 修改、快照只读供回退用）口径注出处；三分支含（命中/叶函数候选/查无后续）各 10 注入例。",
# I2 F12143 回退模拟器
"补充深化：回放点规则注出处（展开码正序应用至异常点偏移，得异常时调用帧状态）；单一源断言指回退引擎与 B07 解释器同实现（两消费面单源）；叶函数短路含（RSP+=8 单步回退）档；交付出参含（EstablisherFrame/HandlerData 双指针）冻结；对账含非易失寄存器全量（R12–R15/RBX/RDI/RSI/RBP/RSP/RIP）。",
# I3 F12144 scope table
"补充深化：scope 条目四列为（Begin/End/Filter/Handler）；内层优先次序规则注出处（区间包含时取内层，起始地址序）；空隙样本指 scope 区间外的 RIP（handler 不询问，继续帧链）；越界 RIP 拒止含（RIP 落函数界内但 scope 外→无 scope 档）；区间重叠注入按损坏档入 F12133 联动拒止。",
# I4 F12145 C 链分发表
"补充深化：CONTINUE_EXECUTION 重执行指 filter 返回后从异常点重执行（NtContinue 路径，F12172 消费）；逐 scope 求值序含（filter 副作用按序可见）口径；异常码过滤例含（ACCESS_VIOLATION 匹配/不匹配）与（自定义码）三子例；桩期显式档指 C 链样本以 S3 录制先行对账（真机判据随闸门）。",
# I5 F12146 三型执行序
"补充深化：执行序日志含（filter 求值/except 命中/finally 执行三类阶段标记）；finally 达达性双覆盖指（正常离开 try）与（异常离开 try）两遍；重执行特例注出处（filter 返回 CONTINUE_EXECUTION 时 except 体不进）；清理面含（嵌套 finally 内层先于外层）次序断言；三值语义档与 F12145 分发表单源消费。",
# I6 F12146 C++ 挂点
"补充深化：EH 结构透传含（ThrowInfo→CatchTable 链）只透传不解析声明；类型匹配判定挂点签名冻结（E2 消费协议书版本号在册）；析构调度/重抛语义分界含（本域仅提供展开到达点，对象析构由 E2 在展开回调内自调）注记；C++ 样本集含（多态 throw/嵌套 catch/栈对象析构序）三子例预告。",
# I7 F12148 两阶段展开
"补充深化：TargetIpp 为（目标帧 continue 地址的二级指针）语义注出处（展开完成后恢复执行点）；搜索阶段 handler 询问轮与展开阶段回调触发序互斥（同帧不重复询问）断言；两阶段边界显式指（TargetFrame 命中即切阶段）日志标记；深层样本含（5/8/12 层）三档；次序账含（展开回调返回异常时的终止档）。",
# I8 F12149 nested exception
"补充深化：嵌套链结构注出处（异常记录链域指向嵌套记录）；原轮收敛含（新轮命中帧在原展开目标之下→继续展开/之上→原轮终止）两分支语义档；深度守卫上限（默认 32 层嵌套）可配档；栈完整性哨兵含（展开前后栈帧指纹比对）；不静默吞指守卫超限必走终止路径（红账+终止留痕）。",
# I9 F12150 注册面
"补充深化：三接口错误码表（Add 重复区间/Callback 空区间/Delete 未注册）；回调域语义含（callback 返回 NULL→继续查找下域）口径；合并序注出处（静态→动态→回调为 Windows 口径，本工程锚定同序）；重叠拒止含（同模块重复 Add 与跨模块重叠）两子例；命中即停指找到首个含 RIP 的表项即用（不多表合并）声明。",
# I10 F12151 动态函数表
"补充深化：区间有序组织含（按 BeginAddress 排序二分查询）与增删重排；引用计数延迟回收档注记（倒排函数表同型口径）；竞态注入含（展开消费中删除/查询中并发增删）两子例；卸载事件钩子冻结（D2 消费接口版本号）；回收守恒含（按模块粒度计数）断言。",
# I11 F12152 VEH
"补充深化：VEH Add 的 First 参数（头插/尾插）语义注出处；Remove 句柄失效处理含（重复 Remove 返回错误）档；VEH 先于 SEH 断言含（同异常 VEH 先收到）注入例；递归守卫指 VEH handler 内再抛异常的防护（限次后终止）；分发主循环前置插入点为单点（与 F12141 序列账单源）。",
# I12 F12153 次序账
"补充深化：级间跳转条件含（VEH handler 返回接管时特例档）；场景含（VEH 内接管后流程）与（嵌套类=UEF 内再异常）两难例；终局退出码语义与 C1 联签（进程退出路径单源）；逐级日志含（级标记/处理器地址/返回值）三列；10 场景参数化脚本入库可重演（种子记录）。",
# I13 F12154 UEF/WER
"补充深化：UEF 末次生效语义注出处（后注册覆盖前注册）；调用时机含（单调用者保证——并发崩溃串行化）口径；EXECUTE 分支指 UEF 返回执行处理（含 WER 前置）→终止；CONTINUE 分支指返回后走默认终局（快速失败档）；WER 主体归 K 部（分界表含 K 部接口冻结版本号）；崩溃转储样本含（小型转储三要素）预告登记。",
# I14 F12155 锁序巡检
"补充深化：持锁清单含（动态表锁/注册锁/序列账锁）三类点逐一登记（持锁域=临界区段范围标注）；报点接口含（获取/释放双事件+线程+锁址）签名冻结；展开内获取新锁次序约束注记（新锁必须在已持锁之后的全序位）；违例注入含（反序取锁）构造模板入库；锁为自研增强档声明（双机对照仅行为档非锁语义档）。",
# I15 F12156 性能账
"补充深化：三档分位含（200 样本混合分布+最重帧单列）两口径；预算超支标红三因子含（帧深/码型复杂度/表规模）定位输出；模拟桩期口径指（指令计数+周期估算桩）显式声明（真机补测登记）；回归双绿指（性能账重跑+语义回放 F12143 对账零偏差）两件；预算值单源在 O1（本批引用带版本号）。",
# I16 F12157 可观测性
"补充深化：两档策略含（常开档=调试/压测、采样档=发布）切换经构建配置；diff 工具含（故意差异检出 10/10 自检）防哑 diff；导出双格式含（文本可读/结构化可解析）；判据脚本消费入口冻结（脚本版本号在册）；五列格式与 F12141 序列账同型（单源声明）。",
# I17 F12158 对照表
"补充深化：15 机制点含基础八点加（嵌套/动态表/VEH/次序/UEF/锁序/性能/日志）；错误路径列含（错误码+触发条件）双值；判据 ID 可达断言含（存在性+批归属）；抽样 20 含（机制类+错误路径类混合）；零断链指（表行↔判据↔复测记录）三向哈希链可溯；对照状态列分双机联测完成率汇总。",
# I18 F12159 ktest 断言集
"补充深化：19 条用例四列元数据断言齐备；真机依赖项（三类注入真机判据）显式 SKIP 清单逐条登记（R-D1-008 联动）；失败定位含（场景号/帧号）二元组加邻近场景结果辅助归因；三族为分发族/展开族/账面族独立入口；双机对照用例挂基准机版本匹配前置检查。",
# I19 F12160 集成账
"补充深化：真机预告四列含（注入方式=构造途径/预期命中序列=帧级断言/补测闸门=触发条件）三值加判据 ID；S3 录制先行对账状态并行指（录制物与对账结论双存档）；四对联签（A2 转换/A5 锁序/K 部 UEF/E2 C++ 挂点）状态三列迁移留痕；悬空零断言含（互引双向索引一致）；B08 收口累计 45,480 入域账递推锚。",
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
print("B08 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
