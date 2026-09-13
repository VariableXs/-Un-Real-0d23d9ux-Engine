# -*- coding: utf-8 -*-
"""UNREAL-X-15000 功能全景图·项级明细生成器。
从旧版全景图解析 600 族元数据，按「五层×五档」模板展开 15000 项逐条明细。
用法：python tools/gen_unrealx.py  （输出覆盖 UNREAL-X-15000-功能全景图.md）
路径安全：仅允许仓库根下两个固定白名单文件，显式 resolve 并校验前缀。
"""
import re, sys, io
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SRC_NAME = 'UNREAL-X-15000-功能全景图.md'
DST_NAME = 'UNREAL-X-15000-功能全景图.md'
ALLOWED = {REPO / SRC_NAME, REPO / DST_NAME}

def safe(name):
    p = (REPO / name).resolve()
    if p not in ALLOWED or not str(p).startswith(str(REPO)):
        raise ValueError('路径越界: %s' % name)
    return p

DOMAINS = [  # (编号, 名称, X起, X止, AI区间, 波次)
    (1, '启动与品牌剧场', 1, 1000, 'AI-01~04', 'W1'),
    (2, '窗口与空间', 1001, 2000, 'AI-05~08', 'W1'),
    (3, '桌面设计·桌面与图标', 2001, 3000, 'AI-09~12', 'W1'),
    (4, '任务栏与开始菜单', 3001, 4000, 'AI-13~16', 'W2'),
    (5, '键盘与输入手感', 4001, 5000, 'AI-17~20', 'W2'),
    (6, '文件与数据能力', 5001, 6000, 'AI-21~24', 'W2'),
    (7, '效率与工具中枢', 6001, 7000, 'AI-25~28', 'W3'),
    (8, '系统集成与硬件', 7001, 8000, 'AI-29~32', 'W3'),
    (9, '兼容性防线', 8001, 8750, 'AI-33~35', 'W3'),
    (10, '安全与隐私', 8751, 9750, 'AI-36~39', 'W4'),
    (11, '开放生态', 9751, 10500, 'AI-40~42', 'W4'),
    (12, '视觉·个性化与氛围', 10501, 11500, 'AI-43~46', 'W4'),
    (13, '声音与通知', 11501, 12250, 'AI-47~49', 'W5'),
    (14, '无障碍与本地化', 12251, 13000, 'AI-50~52', 'W5'),
    (15, 'UI 设计与优化', 13001, 14000, 'AI-53~56', 'W5'),
    (16, '工程质量·性能与收官', 14001, 15000, 'AI-57~60', 'W6-W7'),
]

V_DIR = {1:'oobe',2:'desktop-design',3:'desktop-icons',4:'desktop-design',5:'inputFeel',
         6:'files',7:'tools',8:'hardware',9:'compat',10:'settings',11:'ecosystem',
         12:'vision',13:'sound',14:'a11y-l10n',15:'uikit',16:'quality'}
K_MOD = {1:'bootchain',2:'compositor',3:'display',4:'shell',5:'input',6:'fs',7:'task',
         8:'drivers',9:'compat',10:'sec',11:'mod',12:'gpu',13:'audio',14:'a11y',15:'designsys',16:'checks'}
C_MOD = {1:'boot',2:'spatial',3:'desktop',4:'shell',5:'input',6:'fs',7:'tools',8:'hw',
         9:'compat',10:'sec',11:'eco',12:'vision',13:'audio',14:'a11y',15:'uikit',16:'eng'}

def owner_flags(o):
    return ('内核' in o, 'Variable' in o, '代码分析' in o, '三方' in o)

def loc_for(dom, owner):
    hasK, hasV, hasC, isT = owner_flags(owner)
    if isT and not (hasK or hasV or hasC):
        return '多线协同（精确落点见《AI分工完成图》对应 AI 块）'
    parts = []
    if hasV: parts.append('src/features/%s/' % V_DIR[dom])
    if hasK: parts.append('kernel/varix/src/%s/' % K_MOD[dom])
    if hasC: parts.append('code-analysis/core/src/%s/' % C_MOD[dom])
    return ' + '.join(parts)

def form_for(owner):
    hasK, hasV, hasC, isT = owner_flags(owner)
    if isT and not (hasK or hasV or hasC):
        return '界面 + 服务 + 断言（双线门禁）'
    parts = []
    if hasV: parts.append('设置/界面入口 + 开关与按钮')
    if hasK: parts.append('内核服务接口 + 开关')
    if hasC: parts.append('引擎能力 + CheckSet 注册表断言')
    return '；'.join(parts)

def check_for(owner):
    hasK, hasV, hasC, isT = owner_flags(owner)
    if isT and not (hasK or hasV or hasC):
        return '双线门禁并集（G1~G4）'
    parts = []
    if hasV: parts.append('vitest 断言 + 三主题截图基线（HC 红线）')
    if hasK: parts.append('cargo test + QEMU 启动矩阵')
    if hasC: parts.append('CheckSet 断言 + 基准入 CI')
    return '；'.join(parts)

LAYERS = [
 ('基础实装', [
  '打通「{n}」核心链路：{a}按默认参数完成端到端最小可用闭环，形态可直接操作、可观测',
  '为「{n}」开放{a}全量参数（默认档=现状，不改变既有手感），配置面进设置页并持久化',
  '「{n}」的{a}扩展为完整档位矩阵（≥5 档独立可交付），档间迁移平滑、选择可记忆',
  '「{n}」的配置与数据纳入快照/迁移体系：导出、导入、跨版本携带三通道全通',
  '「{n}」与三线既有功能联调集成验证：无回归、无手感损毁、无视觉体验退化']),
 ('边界与恢复', [
  '「{n}」对非法/极端{a}输入的钳制与护栏：越界回默认、异常不崩溃、给出可读原因',
  '「{n}」失败路径的用户叙事与错误码体系：禁裸报错，每种失败都有下一步建议',
  '「{n}」链路中断后的续跑与状态还原：断点续传/半成品标记/一键续作',
  '「{n}」在 CPU/内存/电量紧张时的资源降级策略与守护开关',
  '「{n}」的回滚路径与卸载净身：不留残档、不残留注册项、可完整撤销']),
 ('手感与细节', [
  '「{n}」全部动效走动效令牌（曲线/时长/缩放三对齐），reduce-motion 自动降级为纯淡入淡出',
  '「{n}」交互面 hover/press/disabled 三态与焦点环逐项过检，像素级对齐设计规范',
  '「{n}」键盘通道全覆盖：焦点序合理、快捷键过冲突检测、roving 语义正确',
  '「{n}」微文案与提示语气统一：中文语境自然、术语一致、长度克制',
  '「{n}」无障碍等价通道：读屏语义完整、对比度达标、提供替代输入方式（HC 红线）']),
 ('性能与优化', [
  '「{n}」建立{a}基准采集与性能预算表，指标入 CI 基线防劣化',
  '「{n}」热路径优化：算法/缓存/批处理三路收益量化，数据入册',
  '「{n}」内存与功耗收敛：待机零增量目标、泄漏检测入长稳',
  '「{n}」低配设备自动降级链：材质/动效/精度三级递降，体验不塌方',
  '「{n}」防劣化回归守卫：断言进注册表只增不删，破坏即红']),
 ('创新拓展', [
  '「{n}」本地启发式智能建议：隐私边界内运行、建议可解释、可一键拒绝',
  '「{n}」批量/自动化模式：脚本入口、批处理队列、进度可观测',
  '「{n}」三线跨域联动场景：与内核/Variable 系统/代码分析的协同用例落地',
  '「{n}」面向创作者/开发者的扩展点：开放接口、示例、文档三件套',
  '「{n}」艺术性表达与彩蛋层：不损主线体验、可关闭、有品牌记忆点']),
]

def main():
    src = safe(SRC_NAME)
    dst = safe(DST_NAME)
    text = src.read_text(encoding='utf-8')
    pat = re.compile(r'- 族(\d{4}) ([^（\n]+)（([^（）]+?) ×25 · ([^）]+?)）X(\d{5})~X(\d{5})')
    fams = []
    for m in pat.finditer(text):
        num, name, axis, owner, s, e = m.groups()
        fams.append(dict(num=int(num), name=name.strip(), axis=axis.strip(), owner=owner.strip(),
                         start=int(s), end=int(e)))
    assert len(fams) == 600, '族解析数=%d' % len(fams)

    out = io.StringIO()
    w = out.write
    w('# UNREAL-X-15000 功能全景图（项级明细完整版）\n\n')
    w('> **Unreal X 计划**：X00001~X15000 共 **15000 项全新功能**，16 大领域 · 600 族 × 25 项，与 NOVA-500（F001~F500）、VARIX-M400、AURORA-10000（F00001~F10000）及仓库全部已交付功能**零重复**。\n')
    w('> **本版为项级明细版**：15000 条逐条给出【层·档】、详细工作内容、交互形态（界面/开关/按钮）、三线落点与验收口径；「形态」栏即独立界面交互与按钮的设计要求，落点栏标明该条属内核 / Variable 系统 / 代码分析 / 多线。\n')
    w('> 25 项结构 = **五层 × 五档**（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展，每层 5 档），模板全文统一、内容按族实例化；层档定义见《UNREAL-X-15000-实施总步骤图》§3.1。\n')
    w('> 领域 15 完整吸收《docs/UI-品质深化完整方案与步骤.md》§0~§18（VS Code 工作台范式 × Win11 系统范式统一为 Variable 设计语言：36 件套 kit、材质五档、数值级规范、逐屏蓝图、动效编排、取色流水线、键位注册表、视觉回归、量化验收），该文档为领域 15 施工规范附册。\n')
    w('> 归属图例：【内核】=kernel/varix ｜【Variable 桌面】=src/ + src-tauri/ ｜【代码分析】=code-analysis/ ｜【三方】=多线协作。三线按项统计各 ≈5000，均衡规则见总步骤图 §1。\n')
    w('> 状态：⬜ 未开始 / 🔶 进行中 / ✅ 完成。当前：**0/15000（规划完成，未开工）**。\n\n')
    w('**归属分布（600 族，按主责）**：Variable 桌面 296 族 · 内核 121 族 · 代码分析 122 族 · 三方 61 族（族内 25 项含跨线联动项，按项统计三线各 ≈5000）。\n\n')
    w('**领域总览**\n\n| # | 领域 | X 区间 | 族 | AI | 波次 |\n|---|------|--------|-----|----|----|\n')
    for d in DOMAINS:
        n_fams = 40 if (d[3] - d[2]) >= 999 else 30
        w('| %02d | %s | X%05d~X%05d | %d | %s | %s |\n' % (d[0], d[1], d[2], d[3], n_fams, d[4], d[5]))

    idx = 0
    for d in DOMAINS:
        w('\n---\n\n## 领域%02d · %s（X%05d~X%05d · %s · %s）\n\n' % (d[0], d[1], d[2], d[3], d[4], d[5]))
        df = [f for f in fams if d[2] <= f['start'] <= d[3]]
        assert len(df) in (30, 40), (d, len(df))
        n_ai = len(df) // 10
        ai_lo = int(d[4].split('~')[0].split('-')[1])
        for gi in range(n_ai):
            ai_no = ai_lo + gi
            grp = df[gi*10:(gi+1)*10]
            w('**AI-%02d %s·第%d组（族%04d~%04d · X%05d~X%05d · %s）**\n\n' % (
                ai_no, d[1], gi+1, grp[0]['num'], grp[-1]['num'],
                grp[0]['start'], grp[-1]['end'], d[5]))
            for f in grp:
                w('- 族%04d %s（%s ×25 · %s）X%05d~X%05d\n' % (f['num'], f['name'], f['axis'], f['owner'], f['start'], f['end']))
                loc = loc_for(d[0], f['owner']); form = form_for(f['owner']); chk = check_for(f['owner'])
                for li, (lname, items) in enumerate(LAYERS):
                    for di, tpl in enumerate(items):
                        idx += 1
                        body = tpl.format(n=f['name'], a=f['axis'])
                        w('  - X%05d 【%s·档%d】%s；形态：%s；落点：%s；验收：%s。\n' % (
                            idx, lname, di+1, body, form, loc, chk))
                w('\n')
    w('---\n\n## ID 唯一性声明\n\n- 本计划 ID 区间 **X00001~X15000**，带 X 前缀，与 NOVA-500、AURORA-10000 的 F 集天然不冲突；本文件 15000 条经生成器算术校验连续无缝无重。\n- 收官时执行族0594「ID 唯一性防线」：全集（F 集 + X 集）unique 校验脚本入库 CI。\n- 本文件由 tools/gen_unrealx.py 生成：修改族定义请改生成器数据源后重新生成，勿手改 15000 条明细（防漂移）。\n')
    dst.write_text(out.getvalue(), encoding='utf-8', newline='\n')
    print('项数:', idx)

if __name__ == '__main__':
    main()
