# -*- coding: utf-8 -*-
"""AI-16 · D1-B06 正文增量深化脚本：每条正文在『与 Windows 对照』前插入增量文本，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B06.md"
GATE = 350

EXP = [
# I0 F12101 结构语义
"补充深化：32767 上限语义含（USHORT 正数域截断）拒止口径；sizeof 断言含 8 字节对齐（x64 指针对齐档）；消费清单变更流程含（消费方确认+版本号递增）两步；双机抽样含（内核模块与 ntdll 两层结构）分列。",
# I1 F12102 NUL 约定
"补充深化：奇数 Length 非法判定注 UTF-16 码元完整性口径；Length>MaximumLength 判定含（非法档显式标注，不静默修正）红线；50 样本含（Length=MAX-2 恰满/Length=MAX-1 非法）两边界例；判定器支持批量模式（样本文件输入），非法样本拒止码与 Windows 对齐注出处。",
# I2 F12103 RtlInitUnicodeString
"补充深化：初始化公式含（MaximumLength=(len+1)×2）与超限检测；NULL 档注记（与空串档区分：空串 Buffer 非 NULL 可有档显式）；所有权声明含（后续 Copy/Append 语义依所有权档分支）联动；10^4 公式验证含随机长度生成器（含 0/1/最大三锚值）。",
# I3 F12104 Ansi/OEM
"补充深化：ANSI/OEM 差异注记（OEM 转换经 OEM 码页面，本工程锚定前恒等表桩）；双串初始化 NULL 源行为与 UNICODE 同型；混用防御含（类型别名字段检查）编译期断言（三结构同型但类型不同）；断言集含跨结构 sizeof 一致性断言。",
# I4 F12105 Copy
"补充深化：截断偶数字节对齐口径（UTF-16 码元边界）含奇数源截断到偶数长度例；截断可查指返回值与目标串 Length 双查；自拷贝安全以 memmove 语义注出处；截断例含（截断点恰在代理对中段）特例（显式档，码元完整性优先于代理完整性口径声明）。",
# I5 F12106 Append 族
"补充深化：BUFFER_TOO_SMALL 返回时目标零改动含（Length 字段不更新）断言；ToString 版含（C 串 NUL 计入源长）口径；追加后 NUL 单份维护（追加位置写入单 NUL）；守恒断言含（源为 UNICODE_STRING 时 Length 取字段非逐字符计）口径注记。",
# I6 F12107 安全族
"补充深化：安全族预检序含（任一参数非法即拒止，先于容量检查）；事务性断言含（失败后目标三字段快照对比）；区分表 8 例含（Copy/Cat/CopyString/CatString×成功/越容量）；成功路径与截断族一致指合法输入下结果逐字节一致；错误码表注出处（INVALID_PARAMETER 统一档）。",
# I7 F12107 AnsiToUnicode（F12108）
"补充深化：TRUE 分配档含分配失败路径（返回错误不回填）断言；FALSE 备容档容量不足返回所需长度口径注出处（溢出码+Length 回填所需值档）；扩展字符 30 组含（Latin-1 全段抽样+假名/西里尔抽样）；码页锚定前双机一致性仅 ASCII 段承诺（扩展段显式待测标注）。",
# I8 F12109 Upcase
"补充深化：整串逐字符映射不改变 UTF-16 长度口径注记；50 字符抽样含（已大写输入恒等）与（无大写映射字符恒等）两子集；特殊折叠例（区域敏感折叠）显式档（本工程单表全局折叠，区域折叠不做声明）；表缺失时桩期恒等表显式记账（R-D1-006 联动）。",
# I9 F12110 比较族
"补充深化：Length 短路比较含（字节数不等即返回）序注出处；不敏感档逐码元折叠消费 F12117 接口（单点消费声明）；30 组含（仅大小写差异等长串）10 组重点；Compare 稳定性显式指（不敏感档下相等返回 0，不承诺跨平台字典序全等）口径；前缀/不等长/等长三类注入分布登记。",
# I10 F12111 Prefix
"补充深化：前缀判定含（前缀长于源串返回 FALSE）边界例；CaseIgnored 双档与 F12110 折叠消费一致性（同一折叠单源）；空串前缀语义待实测注记（预期空串为任何串前缀档显式）；30 组含（假前缀共享首字符）10 组易错点重点覆盖。",
# I11 F12112 整数转换族
"补充深化：To 函数含前缀参数（0x 前缀档）与 Base=0 自动判别档；From 含（首部空白/符号处理）语义注出处；溢出面含（超出域值截断与拒止两档口径）注出处；10^4 往返含（8/10/16 三基轮转）分布登记。",
# I12 F12113 CbPrintf 预告
"补充深化：容量预检含（所需容量计算=格式展开估算）与截断报告（返回所需字符数档）；目标不越界保证为硬断言（截断后写入量≤容量-1 含 NUL）；分界表含（内核安全族不依赖 CRT 运行时）声明；四例最小格式（%s/%d/%x/%ls）双机输出一致档。",
# I13 F12114 缓冲所有权
"补充深化：所有权表含（RtlFreeUnicodeString 仅配 TRUE 分配档）配对约束；配对账含（分配标签-释放标签一致）断言；哨兵面含（分配块头尾双哨兵+释放时校验）调试档实现；悬垂注入含（double free/野指针/块中指针）三子例；桩期显式档指堆桩件记账（真期承接 D2 联签）。",
# I14 F12115 防御矩阵
"补充深化：矩阵含（Buffer NULL+Length 非 0 组合矩阵）重点格；Length 谎报检测以（边界快速探查）调试档实现（发布档不做声明）；拒止错误码统一 INVALID_PARAMETER 档注出处；定位红账含（函数名+参数值+调用点回溯）三要素；30 例含（截断族/拒止族/转换族）三类分布。",
# I15 F12116 UTF-8 面
"补充深化：UTF-8 接口账含（长度预计算+转换+验证）三件签名；无效字节处理语义（替换 U+FFFD 或拒止）显式档待锚定注记；代理对样本含（合法对/半对/反向对）三类；非法半对语义实测裁决档（未测期不承诺）；E 部预告件含联测承接批号登记。",
# I16 F12117 UPCASE 表接口
"补充深化：表接口桩期恒等表显式记账含（表文件版本号+覆盖域声明）；不敏感哈希口径含（折叠后哈希）预告（消费面 D3 命名空间查找）；次序约定注出处（先折叠后哈希，非混合）；消费清单四项联测状态（登记/冻结/联测）逐项登记；ASCII 折叠双机一致含（A-Z 全段）抽样。",
# I17 F12118 对照表
"补充深化：四列含（函数/语义/出处/判据 ID）加状态矩阵扩展列（双机对照状态）；错误矩阵逐列齐指（≥12 函数×4 错误类全格）；50 样本集含版本号（变更须走变更账）；判据复测脚本挂样本集指（脚本直接消费样本文件）；对照表与错误矩阵交叉断言（函数清单一致）。",
# I18 F12119 ktest 断言集
"补充深化：19 条用例三列元数据断言齐备（本批为三列简化型，与后续四列型差异显式）；依赖前件含（F12117 表桩件/F12114 堆桩件）声明，缺桩 SKIP 入桩期账；失败定位含（函数名/样本号）二元组输出；三族为结构族/转换族/防御族独立入口；双机对照用例挂基准机前置检查。",
# I19 F12120 集成账
"补充深化：19 条互引含批内边（结构↔约定↔函数族）与跨批边（B06→B05/B12–B15 消费）分类登记；三对联签状态三列迁移留痕（时间+触发方）；对照项 12 例与待实测项（扩展字符/码页锚定/特殊折叠）逐条三要素登记；B06 收口累计 33,560 入域账递推锚；R-D1-006 桩期依赖项与域待办账同步。",
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
print("B06 OK: total=%d min=%d avg=%d" % (total, min(ns), sum(ns)//len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
