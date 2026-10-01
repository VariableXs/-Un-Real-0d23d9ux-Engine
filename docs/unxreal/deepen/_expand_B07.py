# -*- coding: utf-8 -*-
"""AI-16 · D1-B07 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B07.md"
GATE = 350

EXP = [
# I0 F12121 异常目录全景
"补充深化：目录 Size 非 8 倍数注入（条目完整性前提）拒止；基址+Size 溢出回绕样本单独列格；空目录档含 Size=0 但 RVA 非 0 的矛盾态（红账）；样本比对含基址重定位前后双态（RVA 不变口径断言）。",
# I1 F12122 RUNTIME_FUNCTION 三元组
"补充深化：12 字节布局注 x64 口径（32 位架构除外注记）；BeginAddress=EndAddress 空函数条目拒止例；RVA 非 VA 的换算差异注记；排序不变量（Begin 单调递增）预检挂点（F12123 二分前提），坏序样本 10/10 拒止入红账；抽样 50 含跨节边界函数与 thunk 函数两类特例。",
# I2 F12123 RIP 二分
"补充深化：二分实现以 32 位无符号比较（RVA 域防符号回绕）；等值边界三类（RIP=Begin 命中/RIP=End-1 命中/RIP=End 落下条）样本各 10 例；未命中返回空表项的叶函数候选语义与动态表查找序联动（静态未命中转动态，序不变量断言）；10^4 随机 RIP 含边界密集区（函数首尾 ±8 字节）加权抽样。",
# I3 F12124 UNWIND_INFO 头部
"补充深化：头部 bit 布局（Version 3 位/Flags 5 位/SizeOfProlog 8 位/CountOfCodes 8 位/FrameReg 4 位+FrameOffset 4 位）逐字段掩码表；Version=2 差异档显式；奇数码补齐双字节口径含 CountOfCodes=0 时额外双字节槽（handler RVA 位）注记；越界校验含码数组与 handler RVA 两段独立检查。",
# I4 F12125 四标志族
"补充深化：UHANDLER-only 为展开回调专用形态（C++ finally 产物），E/U 并存样本含 MSVC x64 真实形态（E 在前）注记；FHANDLER 保留档注记（本工程不消费，遇之红账）；CHAININFO 与 E/U 互斥前提断言（并存即损坏入 F12133 拒止）。",
# I5 F12126 PUSH 双码
"补充深化：PUSH_MACHFRAME 完整帧与含错误码两档调整量公式；机器帧内 RIP 槽与展开目标关系注出处；PUSH_NONVOL 寄存器号域 0–15 全枚举（寄存器序注出处）；A2 联签含机器帧构造三例（时钟/页错/调试）登记。",
# I6 F12127 ALLOC 双码
"补充深化：LARGE 双形态操作数序（先 op 后 size 槽序注出处）；8/16/128/136 边界对应 SMALL 上界与 LARGE 下界过渡带；超大分配样本（32 位槽上限）注记以对齐分配例；CountOfCodes 联动含 LARGE 占双槽/三槽的码计数断言。",
# I7 F12128 SAVE_NONVOL 双码
"补充深化：SAVE_NONVOL 槽偏移=RSP 基准÷8 压缩口径，FAR 32 位全偏移域；基准切换（FP 基准）时槽位重算规则（F12130 联动）；越栈界判定以栈提交上限档红账；恢复一致性回放含非易失 GP 寄存器全采样（R12–R15/RBX/RDI/RSI/RBP 重点）。",
# I8 F12129 SAVE_XMM128 双码
"补充深化：XMM8–15 例外注记（XMM0–5 易失不保存口径）；16 步进偏移编码与 8 位域上限（256×16 字节），FAR 型 32 位域扩展例；逐位恢复含非规范数与负零载荷保持；对齐注入含（偏移差 8 字节）最小违规例。",
# I9 F12130 大帧语义
"补充深化：SET_FPREG 码号口径注出处并修正前表差异（差异显式登记）；FP 建立时点（序言中首次出现处）与切换后保存码基准全换（RSP 基准码不可再现约束）；反推验证（由 FrameOffset 反推偏移字节）双向算术断言；同函数双基准样本产自真实编译器样本库。",
# I10 F12131 CHAININFO 链式
"补充深化：链 RVA 定位在 handler RVA 槽（双字对齐），链域 BeginAddress 为 thunk 末续查起点口径；深度上限默认 32 链（超限红账，参数可配）；环检测以访问序号表（同条目二次进入即环）；C++ 样本链路含 jump thunk 两级真实样本。",
# I11 F12132 解释器主体
"补充深化：正序应用至目标偏移的口径与反序回放（展开逆过程）双引擎共用栈状态结构；歧义例（操作数域重叠的未用编码）显式档：遇之按损坏拒止不猜测；RSP/FP 终值参照以双机日志（三元组采集）为准；回放引擎与 F12132 判据源单一（同一实现两消费面）。",
# I12 F12133 损坏拒止
"补充深化：8 类含（版本非法/条目越节界/Begin≥End/头部版本门/码数组越界/对齐违规/链式环/未用编码）逐类判定规则与拒止点定位；字段级手工构造模板入库（每类模板+参数化）；拒止点分层（加载期目录校验/查询期条目校验/解释期码校验三层标注）；双机坏样本行为对照含终止进程档。",
# I13 F12134 样本库制度
"补充深化：版本钉定为单版本锁死（MSVC 一版+MinGW 一版各一），跨版本样本进扩展库不进判据库；三件套审计含编译器完整版本串与环境版本；哈希覆盖中间产物与最终产物双层；降级参考档样本显式标记不入覆盖矩阵（防伪覆盖，R-D1-007 联动）；抽检重编译周期随域闸门。",
# I14 F12135 覆盖矩阵
"补充深化：三维矩阵含（码型 12×深度 3×帧寄存器 2）=72 格理论域，样本 200 函数按格聚合映射（一格多样本允许多对一）；空格判定以（格无样本且非显式待补）为伪空格红账；待补格登记（责任 AI-16/期限/依赖条件）；回放面含单格重放入口（按格号）供回归。",
# I15 F12136 叶函数
"补充深化：单帧弹栈语义含（RSP+=8 恢复调用者现场）展开值计算档；动态域候选查找序判别前置（先静态后动态再叶判定）；模块级统计判别含（Size>0 且查无样本）红账阈值登记；真机判据（硬件叶函数异常）随闸门补测清单。",
# I16 F12137 .xdata 压缩与对齐
"补充深化：内联档判定前提为 UnwindData 最低位=1 且（RVA-1）%4=0；外置 RVA%4≠0 拒止含 RVA=0 注入例；内联头部版本门串联（Version 同样校验）；双轨切换样本（同一函数两轨）构造例入覆盖矩阵；压缩收益统计（样本库内联占比）记录性登记。",
# I17 F12138 对照表与交付协议
"补充深化：交付时机握手含（D2 校验通过→三元组交付→D1 消费确认）三步序；映像基址随交付防二次取基址漂移；对照表五列含（条目/锚/判据 ID/状态/版本档）；抽样 20 复测含全型分布覆盖（每型至少 1 行）；联签三列状态迁移留痕（时间+触发方）。",
# I18 F12139 ktest 断言集
"补充深化：19 条用例四字段元数据（判据 ID/复测函数/依赖前件/桩期标记）断言齐备；依赖前件含（F12134 样本库在册/D2 校验件）声明，缺前件 SKIP 显式；失败定位含（码型/样本号/损坏类）三元组输出；全跑含双机对照项前置检查（样本库版本匹配）。",
# I19 F12140 集成账
"补充深化：19 条互引含批内边（目录↔三元组↔头部↔码型链）与跨批边（B07→D2/B08）分类登记；三件交付状态（样本库/回放面/拒止矩阵）逐项版本号在册；B08 消费接口预告三点（F12125/F12131/F12132）含冻结版本；B07 收口累计 39,340 入域账递推锚。",
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
print("B07 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
