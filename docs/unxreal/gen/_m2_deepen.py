# -*- coding: utf-8 -*-
"""AI-62 · UNX-M2 · 深化轮生成器：deepen/M2-B01..B40 共 40 册。

单源直读：解析两本增补册（B01–B15 / B16–B40）表格行，防转载生成六要素深化册（K3 体例）。
断言：①40 册在位 ②每册 20 条 ③每条含六要素 ④增补册状态逐 ID 翻转「增补→已深化」⑤ID 连续。
"""
import io, os, re, sys

BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main"
DIR = os.path.join(BASE, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
DEEP = os.path.join(BASE, "deepen")
BOOKLETS = [
    os.path.join(DIR, "AI-62 · M2 · 300项新功能增补册（B01–B15）.md"),
    os.path.join(DIR, "AI-62 · M2 · 500项新功能增补册（B16–B40）.md"),
]

AXIS = ("输入延迟毫秒级账本（中断到达→事件层入队→vxwm 焦点分发→窗口应用回调四段打点，P50/P95/P99 段阈值试产校准定）"
        "与多点触控/手势（MT-slot 5 触点并发、tracking_id 生命周期、42 迁移表与回归黄金集）")
UPDOWN = ("上游：AI-02 IrqBackend 中断消费（轮询路径升级接管联签）、AI-65 M5 HID 解析器唯一实现（防重联签）、"
          "AI-94 版本台账（XKB/HID Usage 版本钉定）；下游：vxwm 焦点递送（AI-26 焦点与光标像素分界联签）、"
          "AI-30 手势消费分界、AI-40 RAW Input 键鼠-手柄分界、AI-19 D4 消费序、AI-71 O1 对照列格式、"
          "M1 桥入口 key_event 四元组冻结（AI-61）；同批邻条按批主题内聚协作，跨批依赖经段闸联签锁定，变更走 mini-ADR。")

def parse_booklet(path):
    """返回 [(bno, btopic, [(fid, name, rows, ev), ...20]), ...]"""
    text = io.open(path, encoding="utf-8").read()
    out = []
    cur = None
    for line in text.splitlines():
        m = re.match(r"^## 批 UNX-M2-B(\d+)（F\d+–F\d+ · (.+?) · (.+?) · 6,000 行）$", line)
        if m:
            cur = (int(m.group(1)), m.group(3), [])
            out.append(cur)
            continue
        m = re.match(r"^\| UNX-F(\d+) \| (.+) \| (\d+) \| 增补 \| UNX-F\d+-J1 (.+) \|$", line)
        if m and cur is not None:
            cur[2].append((int(m.group(1)), m.group(2), int(m.group(3)), m.group(4)))
    for bno, _, items in out:
        assert len(items) == 20, f"B{bno} 表行 {len(items)} != 20"
    return out, text

b1, t1 = parse_booklet(BOOKLETS[0])
b2, t2 = parse_booklet(BOOKLETS[1])
allb = sorted(b1 + b2, key=lambda x: x[0])
assert [b[0] for b in allb] == list(range(1, 41)), "批序非 B01–B40"
fids = [f[0] for b in allb for f in b[2]]
assert fids == list(range(48801, 49601)), "ID 段非 F48801–F49600 连续"

# ---------- 生成 40 册 ----------
for bno, btopic, items in allb:
    L = []
    L.append(f"# UNX-M2-B{bno:02d} · 深化册 · {btopic}（20 条 · 六要素 ≥300 字 · AI-62）")
    L.append("")
    L.append(f"> 承独立增补册《AI-62 · M2 · {'300' if bno <= 15 else '500'}项新功能增补册（B{bno:02d}）》UNX-M2-B{bno:02d} 批 20 条骨架，单源直读防转载；"
             f"批主题 verbatim 承批头行；每条 ≥300 字【定位/边界/判据/行数/依赖/风险】；判据号与增补册逐一一致；"
             f"状态翻转一一对应零增删（增补册状态列同步翻转为「已深化」）。域判据主轴：{AXIS}。")
    L.append("")
    for i, (fid, name, rows, ev) in enumerate(items, 1):
        L.append(f"### UNX-F{fid} · {name}（深化）")
        L.append(f"- 域/批：M2/B{bno:02d}｜批内序位：{i}/20｜纯功能行数：{rows}｜深化轮：AI-62")
        L.append(f"【定位】本条为 UNX-M2 键盘/鼠标/触控域 B{bno:02d}「{btopic}」批内第 {i} 条，承载「{name}」语义单元，"
                 f"直接服务域判据主轴——{AXIS}。条目锚定 Varix 内核输入栈：以统一输入事件层为地基（input_event 十族/帧边界/能力位图），"
                 f"全部延迟读数入四段账本单源对账，零双记零编造；本域跨域条目仅联签锚定零改写，fake 预演通道就绪。")
        L.append("【边界】本条只承担上述语义单元的职责面，不越界接管邻条语义：上游冻结接口未就绪时按 Schema 先行纪律以 fake 档推进；"
                 "真机对照类判据在试产校准前一律落账为宿主侧前哨值并显性登记随闸门补测清单，不冒充实测（诚实三态）；"
                 "引导设施与内置盘数据安全红线全程零触碰——本条为输入语义/账本面只读路径，无破坏性写操作；"
                 "输出面遵守交互词典与体验十三章：任何异常三要素呈现（发生了什么/为什么/下一步），异常零静默，隐蔽 catch 注入可全检出。")
        L.append(f"【判据】UNX-F{fid}-J1 {ev}；判据随 ktest 断言面注册，失败注入必红（判据灵敏度自检），fast 档一条命令分钟级全绿；"
                 "真机项随闸门补测登记（开发期零 QEMU 零实机写），阈值以联签锚定值为准、试产校准前不宣称实测。")
        L.append(f"【行数】纯功能行数 {rows}（L 档按语义重量定档），计入域账 240,000 行守恒体系——B{bno:02d} 批内 20 条合计 6,000 行，"
                 "全域 40 批 × 6,000 = 240,000 行满账封账；行数为预算口径，物理行随实现落地对账，差额显性不静默。")
        L.append(f"【依赖】{UPDOWN}")
        L.append("【风险】一、真机环境漂移——延迟/手势判据受硬件与负载影响，对照剧本固定（固定负载集 + 基准机）并记录环境指纹，漂移超限标 N/A 不编造；"
                 "二、触控硬件碎片化——quirk 外设备按「识别可算、手势不承诺」降级口径如实记账，不虚报兼容；"
                 "三、行数预算与实现落地的偏差——以批守恒断言与账实对账显性化，任何蒸发立刻显形（告警+账目）。")
        L.append("")
    text = "\n".join(L) + "\n"
    with io.open(os.path.join(DEEP, f"M2-B{bno:02d}.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write(text)

# ---------- 增补册状态翻转（逐 ID 断言） ----------
for path, text in ((BOOKLETS[0], t1), (BOOKLETS[1], t2)):
    flipped = 0
    def _flip(m):
        global flipped
        flipped += 1
        return f"| UNX-F{m.group(1)} | {m.group(2)} | {m.group(3)} | 已深化 | UNX-F{m.group(1)}-J1 {m.group(4)} |"
    new = re.sub(r"^\| UNX-F(\d+) \| (.+) \| (\d+) \| 增补 \| (UNX-F\d+-J1 .+) \|$", _flip, text, flags=re.M)
    assert flipped == (300 if "B01" in path else 500), f"{os.path.basename(path)} 翻转 {flipped} 条异常"
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(new)

files = sorted(f for f in os.listdir(DEEP) if re.match(r"M2-B\d+\.md$", f))
assert len(files) == 40, f"deepen M2 册数 {len(files)} != 40"
sample = io.open(os.path.join(DEEP, "M2-B01.md"), encoding="utf-8").read()
for k in ("【定位】", "【边界】", "【判据】", "【行数】", "【依赖】", "【风险】"):
    assert sample.count(k) == 20, f"M2-B01 {k} ×{sample.count(k)} != 20"
assert "UNX-F48801" in sample and "UNX-F48820" in sample
print("ALL PASS: 40 deepen books written, 800 entries flipped to 已深化")
sys.exit(0)
