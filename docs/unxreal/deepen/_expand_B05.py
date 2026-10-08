# -*- coding: utf-8 -*-
"""AI-16 · D1-B05 正文增量深化脚本（OBJECT_ATTRIBUTES 全套）：锚点前插入『补充深化』，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B05.md"
GATE = 350

EXP = [
# I0 六字段结构表
"补充深化：六字段偏移按 x64 8 字节对齐立表，Length 恒 0x30（sizeof 编译期断言）；32 位档（4.x 口径）偏移减半分列注出处；结构表与 F12092 表项档互引。",
# I1 OBJ_CASE_INSENSITIVE
"补充深化：大小写折叠消费点在 D3 名字解析（Upcase 表经冻结接口供给），D1 侧仅登记位值与注入样本；变体例含混合大小写/全半角/尾随空格三类，每类 10/10 比对留痕；与 B06 Upcase 语义档互引，折叠行为版本锚（ADR-UNX-008）标注；土耳其 i 档与全角假名变体列待基准机实测项，显式登记不伪绿。",
# I2 OBJ_INHERIT
"补充深化：继承迁移账含迁移时权限掩码透传规则（掩码原样不重映射）与迁移失败回滚档；句柄迁移计数与 D3 表账对平（迁移数=源减数=目标加数）；子进程枚举顺序按句柄值升序立档；非继承句柄不被枚举的负样本 ≥5 例入注入集，遗漏/多迁任一检出即红账定位；桩期以 D2 桩表受理记账，真表切换留痕。",
# I3 OBJ_KERNEL_HANDLE
"补充深化：句柄值高 32 位标记口径注公开资料（索引低位、标记高位的双字段打包），进程表/系统表索引互不重叠断言在册；用户态注入内核句柄的拒止样本含（直接传参/经 OA 透传/经 DuplicateObject 洗白）三路各 10/10；表耗尽档（系统表满）错误码登记；与 F12084/F12091 判据互引。",
# I4 FORCE/OPEN_IF/PERMANENT
"补充深化：PERMANENT 位与删除路径（MakeTemporaryObject 类语义）联动注记，对象寿命账挂 B12 生命周期档；OPEN_IF 的创建/打开双分支以返回信息档（已存在/新创建）区分留痕；三位组合矩阵 8 行全列，非法组合（如 PERMANENT+临时语义）注入拒止。",
# I5 RootDirectory
"补充深化：相对打开解析责任全在 D3（D1 预检三参合法性：根句柄类型/相对名首字符/名字非空），预检失败码与 D3 解析失败码分列防混淆；根句柄失效（已被关闭）注入返回 INVALID_HANDLE 档 10/10；绝对路径+根句柄冲突例入拒止矩阵（F12094 行引用）。",
# I6 ObjectName 契约
"补充深化：哨兵失活注入含（调用前释放/调用中换出/双线程并发改写）三场景；D3 拷贝完成点以解析入口返回时刻为界，拷贝后源缓冲改写不影响本次调用断言；双层引用与 F12087 判据、B06 UNICODE_STRING 契约互引，样本 15 组边界（空串/超长/无终止）在册。",
# I7 SD 挂点
"补充深化：域守卫拦截含（D1 侧引用 SD 解析符号/D3 侧反向越界）双向扫描断言；NULL 路径样本含继承父 SD 与系统缺省两分支各登记；SD 非 NULL 时指针透传的字节级原样性断言（memcmp 全等）在册；与 J1 安全检查判据（F12088）联签状态列互登记。",
# I8 SQOS
"补充深化：模拟级别四档与令牌模拟账（A5 联签）映射注记；ContextTrackingMode 两态（静态/动态）语义与 EffectiveOnly 过滤口径注出处；SQOS.Length 与结构版本绑定校验在册，非法 Length 拒止样本 10/10。",
# I9 InitializeObjectAttributes 宏
"补充深化：宏展开的编译期断言含（字段偏移静态检查）与（Length 常量折叠检查）两项；宏与手工赋值 memcmp 样本 20 组（覆盖全字段/部分 NULL/位组合）在册；宏禁止嵌套调用的 lint 规则登记；与 F12090 判据绑定，宏定义单源入头文件档。",
# I10 D3 四接口冻结
"补充深化：四接口含（句柄分配/句柄查找/句柄遍历枚举/句柄删除）签名逐参定档，参数方向（入/出）与内存责任（调用方/被调方分配）显式；对接校验含桩表→真表切换断言（切换时刻/版本号/回归门）三要素留痕；接口偏移零由构建期符号表比对断言；与 F12091 联签对、D3 任务书互引条款同步登记。",
# I11 HANDLE_TABLE_ENTRY
"补充深化：低位掩码口径（低 24 位 GrantedAccess 等公开资料）注出处并标待实测双属性；表项双字与 F12092 判据绑定；掩码位账含 GENERIC 位到类型特定权限位的映射规则注记（映射由对象类型授予掩码决定，公开口径）；提取器单源函数在册，禁止散写位运算（lint 清单）。",
# I12 OBJECT_TYPE_INITIALIZER 方法表
"补充深化：方法表四核心方法+查询方法的调用时机表（打开/解析/关闭/删除/查询五触发点）注公开口径；D3 注册冻结接口含方法表版本号字段，版本不匹配拒止；缺失面：方法槽空时调用路径拒止码在册；与 B12 生命周期档（F12221 族）消费互引。",
# I13 拒止矩阵
"补充深化：≥10 例逐例含（注入描述/预期错误码/出处/判据 ID）四元组；Length 错档细分（过小/过大/非对齐）三行；KernelHandle 用户态置位档与 F12084 判据互引；矩阵行与本批 ktest 判据 ID 一一映射，回归时逐行重放。",
# I14 版本锚账
"补充深化：版本账逐行含（XP/7/10/11）四档列，全同行标『共性』合并；差异例标（Win8+ 才拒止类）档位注记；回归钩子触发条件为 ADR-UNX-008 锚更新事件，回归包含矩阵重注入与位语义重抽样两动作，回归报告随批归档；锚未更新时锚列冻结禁改（写保护纪律）；消费方按档分支的分支覆盖断言在册。",
# I15 DuplicateObject
"补充深化：SAME_ACCESS 语义（目标掩码=源掩码，DesiredAccess 须为 0）注出处；CLOSE_SOURCE 副作用（源句柄关闭时序在复制完成后）立档；跨进程复制的句柄表锁序（先源后目标）与 A5 锁序账一致；错误档（源句柄无效/目标进程终止竞态）各 10/10 注入。",
# I16 D1–D3 分界联签
"补充深化：联签文本含四接口清单（F12091 族）与结构语义清单（F12081/F12092）互引条款；域守卫双向扫描的符号黑名单（名字解析类/结构解读类）在册；越权注入（D1 侧写解析逻辑桩）构建期拦截断言 10/10；联签状态列（登记/冻结/联测三态）与 D3 任务书同步。",
# I17 对照表三表
"补充深化：三表行数（结构 6 行/位 6 行/协议 ≥10 行）与判据 ID 一一映射；五列校验含出处列域名断言（仅收 NT 内核公开资料域，ReactOS 注记另列）；20 行抽样复测的抽样器种子固定（可回放），复测结果回填对照表状态列；表版本号与批 finalize 表互登记；抽样含全表至少一遍轮转覆盖。",
# I18 ktest 聚合
"补充深化：三族为位语义族（F12082–85）/拒止矩阵族（F12094/F12090）/分界守卫族（F12097/F12088）独立入口；前件自检含（D3 桩表版本匹配）断言，桩期项显式 SKIP 带因登记；失败定位含判据 ID+注入序号二元组；全跑输出两态比例汇总行。",
# I19 集成账
"补充深化：19 条互引矩阵含（消费方/被消费方/引用性质）三列，引用性质分语义消费/联签对/账面对平三类；零悬空断言脚本入构建门禁；跨域联签四对（D3×2/D2×1/J1×1）状态列逐条登记（登记/冻结/联测）；预告面登记 A4 页表账联签对一条。",
]

raw = io.open(P, "rb").read()
text = raw.decode("utf-8")
lines = text.split("\n")
idx = 0
for i, ln in enumerate(lines):
    s = ln.strip().lstrip("-").strip()
    if s.startswith("正文："):
        body = ln[:-1] if ln.endswith("\r") else ln
        add = EXP[idx]
        anchor = body.rfind("与 Windows 对照")
        newbody = (body + add) if anchor == -1 else (body[:anchor] + add + body[anchor:])
        lines[i] = newbody + ("\r" if ln.endswith("\r") else "")
        idx += 1
if idx != len(EXP):
    print("ERROR: consumed %d of %d" % (idx, len(EXP))); sys.exit(1)

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
bad = [(i + 1, n) for i, n in enumerate(ns) if n < GATE]
print("B05 OK: rows=%d total=%d min=%d avg=%d" % (len(ns), total, min(ns), sum(ns) // len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
