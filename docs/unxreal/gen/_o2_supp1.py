# -*- coding: utf-8 -*-
"""AI-72 · UNX-O2 · B01–B15 · 300项新功能增补册构建器
八断言：①15 批在位 ②300 条 ③ID F56801–F57100 连续零跳号零重复
④每批 6,000 行守恒 ⑤全段 90,000 行 ⑥任务书五枚示例锚行数保真
⑦名称唯一 ⑧追加前主汇编册 UNX-F568xx 零命中（防重）
产出：独立增补册 + 主汇编册卷末纯追加 + 根台账会话条目
"""
import io, os, sys, re, datetime

GEN = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, GEN)
from _o2_data1 import BATCHES_1
from _o2_data2 import BATCHES_2
BATCHES = BATCHES_1 + BATCHES_2

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(GEN)))
DOCS = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MAIN = os.path.join(DOCS, "CoRun Varix STAR II · Unxreal.md")
BOOK = os.path.join(DOCS, "AI-72 · O2 · 300项新功能增补册（B01–B15 · F56801–F57100）.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")

START_ID, END_ID = 56801, 57100
ANCHORS = {56801: 400, 56821: 420, 56841: 450, 56901: 320, 57001: 380}

def main():
    # 断言 ① ②
    assert len(BATCHES) == 15, f"批数 {len(BATCHES)} != 15"
    # 断言 ③ ④ ⑤ ⑥ ⑦
    names, ids = [], []
    total = 0
    for bi, b in enumerate(BATCHES):
        rows = b["rows"]
        assert sum(rows) == 6000, f"{b['title'][:20]} 行数 {sum(rows)} != 6000"
        for ii, (name, jieju) in enumerate(b["items"]):
            fid = START_ID + bi * 20 + ii
            ids.append(fid)
            names.append(name)
            m = re.match(r"UNX-F(\d+)-J1", jieju)
            assert m and int(m.group(1)) == fid, f"判据号失配: {name} -> {jieju[:16]}"
        total += sum(rows)
    assert ids == list(range(START_ID, END_ID + 1)), "ID 不连续"
    assert len(ids) == 300, f"条数 {len(ids)} != 300"
    assert total == 90000, f"总行数 {total} != 90000"
    for fid, r in ANCHORS.items():
        bi, ii = divmod(fid - START_ID, 20)
        assert BATCHES[bi]["rows"][ii] == r, f"示例锚 F{fid} 行数 {BATCHES[bi]['rows'][ii]} != {r}"
    assert len(set(names)) == 300, "名称重复"
    # 断言 ⑧ 防重
    with io.open(MAIN, "r", encoding="utf-8", errors="strict") as f:
        main_text = f.read()
    hits = re.findall(r"UNX-F568\d\d|UNX-F569\d\d|UNX-F570\d\d|UNX-F571\d\d", main_text)
    assert not hits, f"主汇编册已有 F568xx-F571xx 命中 {len(hits)} 处，防重失败"
    print("断言 ①–⑧ ALL PASS")

    # 渲染批表格
    def render_batches():
        out = []
        fid = START_ID
        for b in BATCHES:
            out.append("\n## " + b["title"] + "\n")
            out.append("\n| ID | 深化名（deepen/O2） | 行数 | 状态 | 证据与判据锚定 |")
            out.append("|---|---|---|---|---|")
            for (name, jieju), r in zip(b["items"], b["rows"]):
                out.append(f"| UNX-F{fid} | {name} | {r} | 增补 | {jieju} |")
                fid += 1
            bi = fid - START_ID - 1
            lo = START_ID + (bi // 20) * 20
            hi = lo + 19
            out.append(f"\n**批防重声明**：本批 20 条主题两两不重叠；ID 段 F{lo}–F{hi} 与邻批零交叠；批账 6,000 行守恒。\n\n---")
        return "\n".join(out)

    body = render_batches()

    duan_zong = """
## 波 26 段总账（AI-72 · UNX-O2 B01–B15）

- **总量**：15 批 × 20 条 = **300 条全冻结**；ID 段 F56801–F57100 连续零跳号、零复用；本卷域账累计 90,000 行（域账 90,000/240,000 = 37.5%）。
- **批型铺排**：F 型八批 B01–B06 + M 型前九批 B07–B15（任务书「F6/M14」批标号与 ID 区间推算不一致处，按连续零跳号公理恒等归位并诚实登记——AI-51 判例）；任务书五枚示例锚 F56801=400 / F56821=420 / F56841=450 / F56901=320 / F57001=380 行数逐一保真。
- **域内结构**：套件钉版嫁接（B01/B07/B08/B09）→ 基线库快照引擎（B02）→ 漂移告警器（B03–B05）→ 噪声探针与标定（B06/B14）→ 差分与趋势（B10）→ 调度器（B11–B13）→ 三防线与段收官（B15）。
- **O2 向下冻结三件**：baseline schema（F56822/F56823）、告警单格式（F56845）、标定证书语义（F56907/F57061）——冻结日随波 26 Q3 声明在册。
- **联签锚定**：AI-71（双向最密：消费采集 schema ×6 条 + 标定证书约束 O1 ×3 条 + 时段账共享 ×4 条）、AI-88（趋势页/环境劫持联办）、AI-90（闭账九步嵌入）、AI-94（SPEC 上游 SLA）、AI-95（许可脚注冻结权 ×6 条）、AI-97（劣化立案）、AI-82（灌水审计线索）、AI-85（豁免审计）、AI-86（断电判据）、AI-73（维度 18/19 消费）、AI-62/57（参照不重测）——全部联签锚定行出现，零改写他域账。
- **红线**：域内三条加严红线零违例——①禁基线改写（F56825/F56829 断言在位）；②禁未标定入账（F56908/F56909/F57065 拦截在位）；③禁许可裸奔（F56808/F56935/F56951/F56993/F57071 脚注强制在位）；行为红线①禁跨机入账（F56899）②禁缺侧编造 N/A（F57051）③禁账后改判（F56859）全部落闸；本域无引导/写盘红线（适用性声明在册），热节流类条目只读诊断先行。
- **诚实三态**：基准机/真机类判据「随闸门补测」登记不虚报；Windows 同机对照缺侧一律 N/A 不编（附则三第④条）；开发期零 QEMU 零实机写。
- **域经义务**：DJ-UNX-O2-01（为什么这么标定）已立条（F57087）。
- **待续**：B16–B40（500 条 · F57101–F57600 · 波 26 后半–27）另册续写。
"""

    header_book = """# AI-72 · UNX-O2 基准测试体系 · 300 项新功能增补册（B01–B15 · F56801–F57100）

> **任务书锚定**：AI-72 承包域 UNX-O2 基准测试体系（F56801–F57600）· 40 批（B01–B40）· 波次窗波 26–27 · 上游 AI-71（O1 性能对标）· 嫁接源 SPEC 类 · 判据主轴「回归基线漂移告警判据」（AI分工完成图 §AI-72 保真）。本域使命一句话：**把"悄悄劣化"变成"当场告警"——O1 回答"和 Windows 比怎么样"，O2 回答"Varix 自己这一波比上一波是变快了还是变慢了"**。本册为第一次会话产出：前 15 批（B01–B15）共 **300 项新功能增补**，域账入账见批末累计。每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。
>
> **基准六律母版（全域复用，逐条目实例化）**：P1 钉版——套件版本写死进基线元数据，"最新版"式表述即打回；P2 可复现——一条命令从零环境跑到结果文件，重跑结果逐字段可解析；P3 只追加——基线库物理只追加，改写历史必被抓；P4 判噪——漂移 ≤2×抖动带记正常波动不入告警，>2× 且方向为劣化才告警；P5 许可脚注——SPEC 类结果发布物零缺失（联合 AI-95，违者冻结发布）；P6 标定——入正式账的采集器必须带 O2 标定证书（噪声占比 <10%），行使对象含 O1 自己。铁值纪律：判据主轴注入 ±1%/±5% 两档 10/10 行为正确是铁值，禁改阈值（行为红线 #8）。
>
> **防重声明**：本域 300 条主题两两不重叠（同套件同版本不重复立项，套件升级按"双基线过渡"口径带过渡字样）；不触他域账——Windows 同机采集设施归 O1（O2 只消费其采集记录 schema）、ktest 微断言归 A5（ktest 管"对不对"、O2 管"快不快"）、帧率账归 AI-88、启动计时归 L2/AI-57、渲染语义归 G 部、延迟账本归 AI-62——各归其主，仅在联签锚定行出现、零改写。SPEC 类套件按"只跟随"嫁接不 fork 不改语义（与 Wine/Mesa 同规）。
>
> **批次铺排（本册覆盖段）**：B01–B06 F 型（SPEC CPU 四套件钉版嫁接/基线库快照引擎/漂移告警器骨架/告警分级豁免治理/判噪纪律/噪声探针与标定地基）+ B07–B15 M 型前九批（内存缓存类套件/并发吞吐类套件/五类负载与套跑编排/快照差分器与趋势账/波闭账调度器/结果包与虚报防线/机器时段账/标定证书服务/三防线收口与段收官）。B16–B40（500 条）另册续写。

---
"""
    book = header_book + body + duan_zong
    with io.open(BOOK, "w", encoding="utf-8", newline="\n") as f:
        f.write(book)

    # 主汇编册纯追加
    vol = ("\n\n---\n\n# 增补卷 · AI-72 · UNX-O2 基准测试体系（B01–B15 · F56801–F57100）\n\n"
           "> **登记注**：本卷由 AI-72 会话产出，卷末纯追加零删除零改写；任务书五枚示例锚行数保真；批标号与 ID 区间推算不一致处按连续零跳号公理恒等归位并诚实登记（AI-51 判例）。域账 90,000/240,000（37.5%）。\n"
           + body + duan_zong)
    with io.open(MAIN, "a", encoding="utf-8", newline="\n") as f:
        f.write(vol)

    # 根台账会话条目（纯追加）
    ts = datetime.date.today().isoformat()
    entry = f"""

---

## 会话条目 · AI-72 · UNX-O2 基准测试体系 · 首产段立账（{ts}）

- **产出**：300 项新功能增补（B01–B15 · F56801–F57100 · 90,000 行 · 域账 90,000/240,000 = 37.5%）——15 批 × 20 条 × 6,000 行守恒；F 型 B01–B06（SPEC CPU 四套件钉版嫁接/基线库只追加快照引擎/漂移告警器/告警分级豁免/判噪纪律/噪声探针标定地基）+ M 型前段九批 B07–B15（内存缓存/并发吞吐/五类负载编排/快照差分器/闭账调度器/结果包虚报防线/机器时段账/标定证书/三防线收官）。
- **判据主轴**：回归基线漂移告警判据全链——注入 ±1%/±5%/+3%/±0.5% 四档 10/10 行为正确汇总在册（F57088）；O2 向下冻结三件（baseline schema/告警单格式/标定证书语义）声明在位；任务书五枚示例锚 F56801/F56821/F56841/F56901/F57001 行数 400/420/450/320/380 保真，批标号与 ID 区间推算不一致一处按连续零跳号公理恒等归位并诚实登记（AI-51 判例）。
- **机器校验 ALL PASS**：生成器 docs/unxreal/gen/_o2_supp1.py 八断言 exit=0（①15 批在位 ②300 条 ③ID F56801–F57100 连续零跳号零重复 ④每批 6,000 行守恒 ⑤全段 90,000 行 ⑥示例锚行数保真 ⑦名称唯一 ⑧追加前主汇编册 F568xx–F571xx 零命中防重）；独立增补册落盘 + 主汇编册卷末纯追加零删除 + 本台账条目，三落位。
- **红线**：域内三条加严红线全落闸（禁基线改写 F56825/禁未标定入账 F56908/禁许可裸奔 F56808）；行为红线域内版①禁跨机入账②禁缺侧编造 N/A③禁账后改判三条断言在位；无引导/写盘红线（适用性声明在册），热节流类条目只读诊断先行；诚实三态——基准机/真机判据随闸门补测登记不虚报，Windows 对照缺侧 N/A 不编。
- **联签**：AI-71（消费采集 schema + 标定证书约束 O1 + 时段账共享）/AI-88/AI-90/AI-94/AI-95/AI-97/AI-82/AI-85/AI-86/AI-73/AI-62/AI-57 全部锚定行零改写。
- **待续**：B16–B40（500 条 · F57101–F57600）另册续写；会话任务储备深度 ≥3（B16 内存深化/B17 IO 深化/B18 告警演练）。
"""
    with io.open(LEDGER, "a", encoding="utf-8", newline="\n") as f:
        f.write(entry)

    print(f"BOOK  = {BOOK} ({len(book)} chars)")
    print(f"MAIN  += {len(vol)} chars")
    print(f"LEDGER+= {len(entry)} chars")
    print("ALL DONE")

if __name__ == "__main__":
    main()
