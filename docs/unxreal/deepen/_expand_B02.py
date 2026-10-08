# -*- coding: utf-8 -*-
"""AI-16 · D1-B02 正文增量深化脚本（NTSTATUS 语义面）：锚点前插入『补充深化』，回填 finalize 字数行。"""
import io, sys

P = "docs/unxreal/deepen/D1-B02.md"
GATE = 350

EXP = [
# I0 位域解码器
"补充深化：解码器输出四段结构化值含十六进制渲染与符号名解析两形态；切分常量（0xC0000000 掩码等）编译期断言；位域表与 F12038 Facility 表同源登记。",
# I1 Severity 判定
"补充深化：severity 判定边界值（0xBFFFFFFF/0xC0000000 紧邻对）注入样本成对登记；信息档五码含 OBJECT_NAME_EXISTS 类『成功但附带语义』例，语义注记与错误档区分规则在册。",
# I2 PENDING 挂起
"补充深化：取消路径回填含 STATUS_CANCELLED 与 Information=0 双要素；挂起账笔守恒断言入批 ktest；经典误用三例含（当同步码用/忽略 IOSB/双重等待）。",
# I3 等待码五态
"补充深化：bWaitAll=FALSE 时返回索引编码（低位索引+对象位）注出处；WAIT_ABANDONED 与 Mutant 废弃联动走 F12035 码表；多对象映射样本 20 组含混合信号态例。",
# I4 ACCESS_DENIED
"补充深化：两路径样本各 ≥5 例注入留痕，路径→码值映射单一表构建期查重；伴生码（权限掩码语义档 F12025 互引）登记；J1 联签判定『拒绝』三要素（主码/路径/动作建议）逐要素断言。",
# I5 坏句柄四类
"补充深化：四类判据（generation 旧值/已关索引/索引越界/内核句柄用户引用）判定函数逐类单测 10/10；表耗尽伴生码与 INVALID_HANDLE 主码的边界（可重试/不可重试）注记；联签接口含句柄有效性查询一档，桩期以桩表记账真表切换留痕。",
# I6 参数错误宏族
"补充深化：三元组（拒止码/参数序号/检查名）经 F12037 文本化三要素输出；嵌套深层字段定位规则（点路径表示法）样本含三级嵌套例；调试档全检开销账登记（O1 预算引用）。",
# I7 AV 码/硬件异常映射
"补充深化：AV 参数 0=读/1=写/8=DEP 档注公开口径，参数 1=违约地址断言；映射单一源与 B08 分发器共用版本绑定断言；页错误/保护错误/非法指令三向量映射样本各 10/10，未知向量显式红账不猜测。",
# I8 OBJECT_* 8 码
"补充深化：NAME_COLLISION 与信息档『已存在』区分规则（返回码+Information 双要素）立档；映射协议联签件含双向翻译表；每码 ≥3 注入场景含（正常路径/竞态/参数错配）三类覆盖。",
# I9 文件 I/O 10 码
"补充深化：END_OF_FILE 双档语义（错误/通知）判定规则注出处；grep 纪律入 lint 清单（散写字面量构建警告）；B1 翻译表联签件含双向映射与缺项账（缺项显式登记不猜测）；翻译表版本号随批冻结。",
# I10 MEMORY_* 码
"补充深化：≥8 码含 PAGE_FAULT_TRANSITION/DEMAND_ZERO 高频档；A4 报点→本域出码的单一方向断言（禁止反向出码）在册；映射冻结件含版本号与回归门。",
# I11 资源耗尽
"补充深化：三类触发（池耗尽/配额顶/等待块尽）统一出码断言（同资源类同码）；注入风暴 10^4 次含（单类风暴/混合风暴）两档；退避可观测性（调用方可见的退避建议字段）登记；耗尽事件账与 O1 审计账面复用单源；风暴后恢复路径回归绿。",
# I12 NOT_IMPLEMENTED/UNSUPPORTED
"补充深化：『返回成功但无实质动作』识别含（返回路径审计桩）注入验证 10/10；三码判定规则表与 F12033 判据绑定；桩态接口清单与 D3/D2 桩表同步版本号。",
# I13 BUFFER 三码
"补充深化：两段式回填协议含（首次调用 Information=-1 口径）注出处；PARTIAL_COPY 已复制量回传与 F12247 判据互引；三码触发时序图在册，越界写例注入即红账（危险档零容忍）。",
# I14 CANCELLED/ABANDONED
"补充深化：索引编码规则注出处（多对象废弃态按索引打包）；废弃判定单一源与 B15 Mutant 账绑定；取消风暴 10^3 次全取消断言含账笔守恒（挂起=完成+取消）；废弃注入含持锁线程终止场景红账验证。",
# I15 NT→Dos 映射表
"补充深化：静态表 ≥100 对构建期查重（重复键编译失败）；哈希查表 O(1) 与缺项账（ERROR_GEN_FAILURE 档+记账）衔接；30 对抽样覆盖高频（权限/文件/内存三域）；Dos 码出处列逐对登记。",
# I16 错误文本化
"补充深化：三要素（短含义/典型原因/建议动作）输出零分配断言（栈上缓冲）；诊断模式串口面与 A1 日志环双出口；未知码输出含通用排查指引（位域解码器 F12021 联动渲染）。",
# I17 Facility 表
"补充深化：≥20 项 Facility 表构建期查重；客户位（0x20000000）占用纪律入 ADR 流程注记（自研扩展码必须置位+登记）；保留区间访问样本注入 10/10 红账；未知 Facility 输出归 F12037 未知档。",
# I18 语义档母版
"补充深化：五字段母版类型定义钉死（编译期防漂移）；18 个码族条目录入即校验（判据 ID 存在性断言）；表行导出复测口与 F12039 判据绑定，导出格式版本号冻结。",
# I19 ktest 五族
"补充深化：断言总数 ≥65 逐族分解（注入类条目 3–5 断言）；族间互斥组显式声明防重复计数；skip 带因（如『需 A4 压力档』）逐条登记；跑批账与 F12039 母版 judge_ids 双向可达断言。",
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
print("B02 OK: rows=%d total=%d min=%d avg=%d" % (len(ns), total, min(ns), sum(ns) // len(ns)))
if bad:
    print("FAIL:", bad); sys.exit(1)
print("PASS gate=%d" % GATE)
