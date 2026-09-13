# -*- coding: utf-8 -*-
"""UNREAL-X-15000 全景图逐条展开生成器。
从现有全景图解析 600 族元数据（族号/族名/轴/归属/AI/波次），按 §3 五层×五档模板
为每族生成 25 条独立条目行；领域15（族0521~0560）使用手写数据文件逐条拼接。
"""
import re, sys, io, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
SRC = ROOT / "UNREAL-X-15000-功能全景图.md"
D15 = [ROOT / f"tools/gen_unrealx/d15_ai{n}.txt" for n in (53, 54, 55, 56)]
OUT = ROOT / "UNREAL-X-15000-功能全景图.new.md"

# 五层 × 五档槽位（与《实施总步骤图》§3 模板一一对应）
SLOTS = [
    ("基础", ["骨架实现", "主路径打通", "数据流接线", "入口与开关", "验收断言"]),
    ("进阶", ["边界与异常", "失败恢复", "降级链", "兼容共存", "回归防护"]),
    ("细节", ["手感参数", "动效曲线", "视觉像素", "文案与提示", "状态完备"]),
    ("优化", ["性能预算", "功耗资源", "内存缓存", "并发竞态", "基线对比"]),
    ("创新", ["智能化", "自动化", "跨线联动", "彩蛋时刻", "开放扩展"]),
]

FAM_RE = re.compile(
    r"- 族(\d{4}) ([^（\n]+)（([^）]*)）\s*(X\d{5})~(X\d{5})")

def parse_families(text):
    fams = []
    for m in FAM_RE.finditer(text):
        num, name, meta, xs, xe = m.groups()
        # meta 形如 "链路段 ×25 · 内核" 或 "§10.1 grid · 区段 ×25 · Variable 桌面"
        axis = meta.split("×25")[0].strip(" ·.")
        attr = meta.split("·")[-1].strip()
        fams.append(dict(num=num, name=name.strip(), axis=axis, attr=attr, xs=xs, xe=xe))
    return fams

def gen_family_lines(f):
    """为一般族生成 25 条目行（领域15 族由手写数据提供）。"""
    base = int(f["xs"][1:])
    lines, n = [], 0
    for layer, slots in SLOTS:
        for slot in slots:
            n += 1
            xid = f"X{base + n - 1:05d}"
            if slot == "跨线联动":
                desc = f"与另两线接口联动，联动落点见分工图（{f['attr']}主责）"
            else:
                desc = f"{slot}：{f['axis']}维度独立可交付档，参数档 {n}/25"
            lines.append(f"- {xid} {layer}·{slot} — {f['name']}：{desc}")
    return lines

def main():
    text = SRC.read_text(encoding="utf-8")
    fams = {int(f["num"]): f for f in parse_families(text)}
    assert len(fams) == 600, f"族解析数 {len(fams)} != 600"

    # 手写领域15 数据
    d15_blocks = {}
    for p in D15:
        cur = None
        for line in p.read_text(encoding="utf-8").splitlines():
            m = re.match(r"### 族(\d{4})", line)
            if m:
                cur = int(m.group(1)); d15_blocks[cur] = [line]
            elif cur and line.strip():
                d15_blocks[cur].append(line)
    assert len(d15_blocks) == 40, f"领域15 手写族数 {len(d15_blocks)} != 40"
    for num, blk in d15_blocks.items():
        items = [l for l in blk if l.lstrip().startswith("- X")]
        assert len(items) == 25, f"族{num} 手写条目 {len(items)} != 25"

    out = io.StringIO()
    w = out.write
    w(text.split("---\n\n## 领域01")[0].rstrip() + "\n\n")
    w("> **逐条版说明**：本版每族 25 项全部展开为独立条目行；一般族条目按《实施总步骤图》§3 五层×五档模板展开（每条 = 独立可交付单元 + 验收断言），领域15 族0521~0560 为逐条手写规格（落实《docs/UI-品质深化完整方案与步骤.md》§0~§18）。\n")
    w("> 归属图例：【内核】=kernel/varix ｜【Variable 桌面】=src/ + src-tauri/ ｜【代码分析】=code-analysis/ ｜【三方】=跨线组合。\n\n---\n")

    dom_meta = [  # (领域标题, 起族, 止族, AI 标头)
        ("领域01 · 启动与品牌剧场（X00001~X01000 · AI-01~04 · W1）", 1, 40, {0: "AI-01 启动可靠与恢复", 1: "AI-02 电源状态剧场", 2: "AI-03 品牌剧场深化", 3: "AI-04 启动收官与遥测"}),
        ("领域02 · 窗口与空间（X01001~X02000 · AI-05~08 · W1）", 41, 80, {0: "AI-05 窗口几何学", 1: "AI-06 空间管理", 2: "AI-07 内核窗口引擎", 3: "AI-08 空间分析"}),
        ("领域03 · 桌面设计·桌面与图标（X02001~X03000 · AI-09~12 · W1）", 81, 120, {0: "AI-09 图标体系", 1: "AI-10 壁纸与微件", 2: "AI-11 内核桌面服务", 3: "AI-12 桌面设计与分析"}),
        ("领域04 · 任务栏与开始菜单（X03001~X04000 · AI-13~16 · W2）", 121, 160, {0: "AI-13 任务栏形态与交互", 1: "AI-14 开始菜单", 2: "AI-15 浮层系统", 3: "AI-16 任务栏内核与引擎"}),
        ("领域05 · 键盘与输入手感（X04001~X05000 · AI-17~20 · W2）", 161, 200, {0: "AI-17 输入手感面", 1: "AI-18 输入智能", 2: "AI-19 内核输入栈", 3: "AI-20 输入工程与中文"}),
        ("领域06 · 文件与数据能力（X05001~X06000 · AI-21~24 · W2）", 201, 240, {0: "AI-21 文件管理面", 1: "AI-22 数据能力面", 2: "AI-23 内核文件系统", 3: "AI-24 数据智能与收官"}),
        ("领域07 · 效率与工具中枢（X06001~X07000 · AI-25~28 · W3）", 241, 280, {0: "AI-25 效率工具面", 1: "AI-26 生活工具面", 2: "AI-27 工具智能与联动", 3: "AI-28 工具内核与收官"}),
        ("领域08 · 系统集成与硬件（X07001~X08000 · AI-29~32 · W3）", 281, 320, {0: "AI-29 硬件体验面", 1: "AI-30 系统服务面", 2: "AI-31 内核硬件栈", 3: "AI-32 设备场景与收官"}),
        ("领域09 · 兼容性防线（X08001~X08750 · AI-33~35 · W3）", 321, 350, {0: "AI-33 运行时兼容", 1: "AI-34 兼容工程", 2: "AI-35 兼容深化与收官"}),
        ("领域10 · 安全与隐私（X08751~X09750 · AI-36~39 · W4）", 351, 390, {0: "AI-36 隔离与沙盒", 1: "AI-37 隐私与身份", 2: "AI-38 防线工程", 3: "AI-39 安全深水区与收官"}),
        ("领域11 · 开放生态（X09751~X10500 · AI-40~42 · W4）", 391, 420, {0: "AI-40 生态面", 1: "AI-41 生态治理", 2: "AI-42 生态工程与收官"}),
        ("领域12 · 视觉·个性化与氛围（X10501~X11500 · AI-43~46 · W4）", 421, 460, {0: "AI-43 个性化深化", 1: "AI-44 视觉系统", 2: "AI-45 视觉内核与质量", 3: "AI-46 视觉生态与收官"}),
        ("领域13 · 声音与通知（X11501~X12250 · AI-47~49 · W5）", 461, 490, {0: "AI-47 声音设计面", 1: "AI-48 通知与节拍", 2: "AI-49 声音内核与收官"}),
        ("领域14 · 无障碍与本地化（X12251~X13000 · AI-50~52 · W5）", 491, 520, {0: "AI-50 无障碍五域", 1: "AI-51 人群与区域", 2: "AI-52 无障碍内核与收官"}),
        ("领域15 · UI 设计与优化（X13001~X14000 · AI-53~56 · W5）", 521, 560, {0: "AI-53 工作台范式与 kit 基础", 1: "AI-54 kit 系统件与系统范式", 2: "AI-55 主题流水线与视觉回归", 3: "AI-56 UI 质量收官"}),
        ("领域16 · 工程质量·性能与收官（X14001~X15000 · AI-57~60 · W6-W7）", 561, 600, {0: "AI-57 工程基建", 1: "AI-58 内核与引擎质量", 2: "AI-59 协作与防线", 3: "AI-60 大收官"}),
    ]
    for title, f1, f2, ai_map in dom_meta:
        w(f"\n## {title}\n\n")
        for gi, ai_name in ai_map.items():
            gf1 = f1 + gi * 10
            gf2 = gf1 + 9
            xs = fams[gf1]["xs"]; xe = fams[gf2]["xe"]
            w(f"**{ai_name}（族{gf1:04d}~{gf2:04d} · {xs}~{xe}）**\n\n")
            for num in range(gf1, gf2 + 1):
                f = fams[num]
                if num in d15_blocks:
                    w("\n".join(d15_blocks[num]) + "\n")
                else:
                    w(f"族{num} {f['name']}（{f['axis']} ×25 · {f['attr']} · {f['xs']}~{f['xe']}）\n")
                    w("\n".join(gen_family_lines(f)) + "\n")
                w("\n")
    w("---\n\n## ID 唯一性声明\n\n- 本计划 ID 区间 **X00001~X15000**，600 族连续无缝、每族恰 25 项；收官时执行族0594 全集差分校验（与 F 集零交集）。\n")
    OUT.write_text(out.getvalue(), encoding="utf-8")
    print("生成完成:", OUT, len(out.getvalue()), "bytes")

if __name__ == "__main__":
    main()
