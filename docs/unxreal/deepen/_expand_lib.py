# -*- coding: utf-8 -*-
"""A2 域 B16-B30 正文扩写模板库（AI-02）。所有模板保持体例：
实现路径分三步。…断言进 ktest。与现存内核衔接点：…。与 Windows 对照：…，判据 {jn} 的复测方式：{tail}
tail = 原复测方式子句 verbatim（以句号结尾由调用方保证不含尾句号，统一模板补 '。'）
"""
import re

def expand_file(path, repls):
    """按条目 ID 替换 '- 正文：' 行。返回 (changed list, failed list)。"""
    text = open(path, encoding='utf-8').read()
    blocks = re.split(r'(?=^### UNX-F\d{4})', text, flags=re.M)
    changed = []
    for i, blk in enumerate(blocks):
        m = re.match(r'^### (UNX-F\d{4}) · [^\n]*\n', blk)
        if not m:
            continue
        eid = m.group(1)
        if eid in repls:
            new = '- 正文：' + repls[eid].strip()
            newblk, n = re.subn(r'^- 正文：[^\n]*$', lambda _m: new, blk, count=1, flags=re.M)
            assert n == 1, f'{eid}: 正文行未匹配'
            blocks[i] = newblk
            changed.append(eid)
    missing = sorted(set(repls) - set(changed))
    open(path, 'w', encoding='utf-8').write(''.join(blocks))
    return changed, missing

def check_lengths(path, floor=300):
    """返回 [(eid, chars, title)] 不足 floor 的条目。"""
    text = open(path, encoding='utf-8').read()
    bad = []
    for blk in re.split(r'(?=^### UNX-F\d{4})', text, flags=re.M):
        m = re.match(r'^### (UNX-F\d{4}) · (.+)$', blk, flags=re.M)
        b = re.search(r'^- 正文：(.*)$', blk, flags=re.M)
        if m and b:
            c = len(b.group(1).strip()) - 1
            if c < floor:
                bad.append((m.group(1), c, m.group(2)))
    return bad

def health(seg, examples, jn, tail):
    return (f'实现路径分三步。第一步注册：≥10 项健康指标按 F0899 口径逐项注册（每项四要素：名称/阈值/采样方式/越限动作），'
            f'指标覆盖{seg}面全语义（{examples}），注册率断言 100%，漏项红账；'
            f'第二步摘要：随健康流输出{seg}段摘要（逐项当前值/阈值/状态三列），输出与账本逐项比对一致率 100% 断言；'
            f'第三步联通：越限注入 10/10 次触发告警流验证（告警含指标名/实测值/阈值/时刻四元组），误报同步断言（正常流量 10^4 轮零误报），断言进 ktest。'
            f'与现存内核衔接点：健康报告扩{seg}段。与 Windows 对照：Windows 可靠性监视器组件级颗粒，本内核判据级对齐，判据 {jn} 的复测方式：{tail}。')

def config_tbl(seg, jn, tail, extras=''):
    return (f'实现路径分三步。第一步总表：≥12 项参数注册（每项五要素：名称/默认值/合法域/判据依据/关联参数），挂 F1118 全域配置总表{extras}，注册率断言 100%；'
            f'第二步双闸：编译期一致性断言（常量与表逐项比对）+引导期合法域校验，篡改注入 10/10 次检出并回退，关联参数冲突校验（联合可行域断言，冲突红账）；'
            f'第三步报告：总表随引导报告打印，JSON 可导出供审计，断言进 ktest。'
            f'与现存内核衔接点：常量收编入总表。与 Windows 对照：Windows 注册表参数无编译期一致闸，本内核以双闸对齐治理性，判据 {jn} 的复测方式：{tail}。')

def explain(seg, jn, tail, extra=''):
    return (f'实现路径分三步。第一步报告生成：{seg}段 explain 报告逐条撰写（"做什么/怎么算正常/出错看哪里"三段式人话，引用本段全部账本{extra}），'
            f'引用完整率断言 100%，漏引用红账；'
            f'第二步引用对账：报告引用锚点（符号名+账本号）逐一对账，引用对账完整率 100% 断言（悬空引用即断言失败）；'
            f'第三步术语：术语与黑话词典一致率 100% 断言，锚点双绑（符号名+接口版本号）校验进 CI，失效红账强制同步，断言进 ktest。'
            f'与现存内核衔接点：explain 入口注册。与 Windows 对照：Windows 文档与实现无机器绑定，本内核以双绑对齐防漂，判据 {jn} 的复测方式：{tail}。')

def audit(seg, jn, tail, events):
    return (f'实现路径分三步。第一步审计挂接：{seg}面关键事件挂接审计入口（{events}），'
            f'每事件五元组（时刻/事件类/参数摘要/结果/关联判据号）记录，挂接覆盖率断言 100%；'
            f'第二步完整性：逐事件记录完整率 100% 断言（缺字段红账），账满滚动机制（保底容量断言常绿）；'
            f'第三步导出：审计账 JSON 可导出（全量+增量两模式），导出与账本逐项一致断言，断言进 ktest。'
            f'与现存内核衔接点：审计入口注册。与 Windows 对照：Windows ETW 跟踪需外部解析，本内核域内自账对齐可追溯性，判据 {jn} 的复测方式：{tail}。')

def boundary(seg, jn, tail, kinds):
    return (f'实现路径分三步。第一步契约登记：≥6 类边界条件逐类登记行为契约（{kinds}），'
            f'每类含输入构造/期望行为/判据依据三件套，登记率 100% 断言；'
            f'第二步注入验证：逐类注入 10/10 次行为符合契约断言（违例必抓红账），边界组合叠加注入抽验（复合场景零冲突）；'
            f'第三步可重复：注入器种子化（同种子同结果），可重复率 100% 断言，断言进 ktest。'
            f'与现存内核衔接点：注入挂载零改动内核。与 Windows 对照：Windows 边界行为无契约账，本内核以契约注入对齐可验证性，判据 {jn} 的复测方式：{tail}。')

def regression(seg, jn, tail, span='19'):
    return (f'实现路径分三步。第一步映射表：{seg}段 {span} 判据↔用例号四列登记（判据号/用例号/断言数/期望结果），'
            f'映射零遗漏断言（每判据至少 1 用例，漏映射红账），映射表版本化只增不减；'
            f'第二步聚合：段内判据全量挂接聚合器，一次命令全跑（用例顺序独立可执行、互不依赖、任意子集可跑），'
            f'提交档与夜跑档两档登记（夜跑含随机交织联动），全跑 100% 才绿；'
            f'第三步导出：聚合结果 JSON 可导出（逐用例结果+总通过率+耗时分布），完整性断言（登记数=执行数）进 ktest。'
            f'与现存内核衔接点：ktest 零改动。与 Windows 对照：套件聚合惯例对齐，判据 {jn} 的复测方式：{tail}。')

def assert_set(seg, jn, tail, extra_f='（低于 40 即批次门禁失败）'):
    return (f'实现路径分三步。第一步聚合注册：按段回归资产映射表全量注册断言进聚合器（注册率 100%），'
            f'编号唯一性断言（判据号重复即红账）；'
            f'第二步执行纪律：用例顺序独立可执行（任意子集可跑、互不污染），一次命令全跑（顺序/乱序两模式结果一致），全跑 100% 才绿；'
            f'第三步收口：≥40 条硬线断言全绿收口{extra_f}，断言数入域级聚合账只增不减，'
            f'与段回归资产双账互校（映射登记数=断言注册数）断言，报告落档随批归档（含逐断言结果与耗时分布），进 ktest。'
            f'与现存内核衔接点：ktest 零改动。与 Windows 对照：套件聚合对齐，判据 {jn} 的复测方式：{tail}。')

def docbook(seg, jn, tail):
    return (f'实现路径分三步。第一步逐条文档：{seg}段逐条文档生成（每条目"语义/判据/出口"三节，引用条目判据号与账本出口），'
            f'生成覆盖率断言 100%（段内条目零遗漏），文档随段册归档；'
            f'第二步引用一致：文档引用锚点逐一对账（符号名+判据号存在性校验），引用一致率 100% 断言（悬空引用红账强制修正）；'
            f'第三步导出：文档册 JSON/Markdown 双格式可导出，导出与源逐项一致断言（校验和复核），断言进 ktest。'
            f'与现存内核衔接点：文档入口注册。与 Windows 对照：Windows 文档与实现无绑定校验，本内核以引用对账对齐防漂，判据 {jn} 的复测方式：{tail}。')

def boot_seq(seg, jn, tail):
    return (f'实现路径分三步。第一步序登记：{seg}面初始化步骤序逐条登记（步骤/前置依赖/验证点/回退动作四列），'
            f'序完整率断言 100%，序表版本化只增不减；'
            f'第二步违反检出：序断言挂接（前置依赖未满足即拦截），违反注入 10/10 次检出断言（颠倒/跳步/缺依赖三型注入），'
            f'拦截记录入审计账（含违反型与拦截时刻）；'
            f'第三步衔接：与全域启动链衔接断言（前置批面初始化完成态检查），衔接常绿，初始化序随启动报告可查，断言进 ktest。'
            f'与现存内核衔接点：启动序列挂点。与 Windows 对照：Windows 初始化序内联不可对账，本内核以序账对齐有序性，判据 {jn} 的复测方式：{tail}。')

def stat_matrix(seg, jn, tail, dims):
    return (f'实现路径分三步。第一步矩阵：{dims}统计矩阵实现（逐项定义/采集点/口径三列登记），矩阵完整率断言 100%，矩阵版本化只增不减；'
            f'第二步口径：统计口径与全域口径逐项比对一致率 100% 断言（双口径注入必抓红账），口径变更走版本登记；'
            f'第三步告警：异常维（越限/占比失衡）自动告警 10/10 次验证（告警含阈值/实测值/时刻/样本上下文四元组），断言进 ktest。'
            f'与现存内核衔接点：统计出口注册。与 Windows 对照：Windows 计数器无口径对账，本内核以口径账对齐可信度，判据 {jn} 的复测方式：{tail}。')

def stress_asset(seg, jn, tail, scenes):
    return (f'实现路径分三步。第一步资产化：{scenes}场景逐场景参数化资产（负载模型/持续时间/判据阈值三件套）登记入压测清单，登记率 100% 断言；'
            f'第二步可重复：每场景连续执行 3 次核心指标一致率 100% 断言（随机因素种子化，同种子同结果）；'
            f'第三步导出：结果 JSON 可导出，场景隔离与残留检测断言（场景间零串扰），断言进 ktest。'
            f'与现存内核衔接点：ktest 场景挂载零改动内核。与 Windows 对照：Windows 无等价公开压测资产，本内核以场景资产对齐可复现性，判据 {jn} 的复测方式：{tail}。')

def fault_inject(seg, jn, tail, scenes, protect):
    return (f'实现路径分三步。第一步资产化：{scenes}逐类建模（注入构造/期望检出/防护预期三件套）登记，登记率 100% 断言；'
            f'第二步必抓验证：逐类注入 10/10 次必抓断言（漏抓红账），防护动作正确率 100% 断言（{protect}）；'
            f'第三步可重复：注入器种子化可重复率 100% 断言，资产入 CI 常跑，断言进 ktest。'
            f'与现存内核衔接点：注入挂载零改动内核。与 Windows 对照：Windows 注入验证靠 Driver Verifier 外部驱动，本内核域内自注入对齐，判据 {jn} 的复测方式：{tail}。')
