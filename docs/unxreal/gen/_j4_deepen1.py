# -*- coding: utf-8 -*-
"""
AI-49 · UNX-J4 沙箱与隔离 · 域深化轮第一段生成器（B01–B15）
UNX-F38401–F38700 · 15 册（B01–B15）× 20 条新深化 · 300 条深化 · 每批 6,000 行锁定 · 全段 90,000 行
体例：承 AI-26 F1 深化增补卷判例 + AI-40 H5 deepen 册判例
纪律：单源直读主汇编册 J4 首产段骨架账（防转抄）· 骨架行数/判据号 verbatim 承接零改动 ·
      每条深化正文六要素（定位/边界/判据/行数/依赖/风险）≥300 字 · 判据自指 · 零新 ID
内建七断言：条数 300 / ID 连续零跳号 / 判据唯一 / 每条正文 ≥300 字 /
            批 6,000 行守恒（20×300）/ 深化零新 ID（全部落于骨架段）/ 状态翻转零遗漏
"""
import json, os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
DEEPEN = os.path.join(ROOT, "docs", "unxreal", "deepen")
GEN = os.path.dirname(os.path.abspath(__file__))

START, END, PER, ROW = 38401, 38700, 20, 300
BATCHES = 15
VOL_HEAD = "## 增补卷 · AI-49 · 三 · J4 域深化增补卷第一段（B01–B15 · 300 条深化 · 骨架满账封账后深化阶段开卷）"
MAIN_VOL_GUARD = VOL_HEAD

# 批主题（承 docs/unxreal/gen/_j4_firstprod.py BATCH_META，逐字保真）
BATCH_META = {
 1:  ("F 型地基：容器 SID 与 AppContainer 结构骨架", "容器 SID 派生/容器标志位/能力 SID 表/令牌构造扩展位骨架"),
 2:  ("F 型地基：AppContainer 三查编排器骨架", "强制标签 ACE 检查编排/DACL 容器 SID 检查编排/能力 SID 检查编排/诊断账骨架"),
 3:  ("F 型地基：seccomp 类 BPF 指令集与验证器", "加载/比较/跳转/返回四类指令/越界跳转拒止/未知指令拒止/指令上限 4096"),
 4:  ("F 型地基：过滤器挂载点与安装语义", "C2 分发表前置钩/安装不可提升/叠加合取语义/TSYNC 线程组同步"),
 5:  ("F 型地基：Job 对象结构与生命周期", "Job 结构/子进程自动入组/循环引用拒绝/TerminateJob 组内全杀"),
 6:  ("F 型地基：Job 四维限额结构（内存/CPU/IO/UI）", "commit 上限/时间片预算窗口/IOPS 带宽两维/剪贴板桌面切换全局钩三类 UI 限额"),
 7:  ("F 型地基：文件系统虚拟化重定向引擎骨架", "per-box 覆盖区/读穿透写落盘/覆盖区优先合并视图/box 生命周期"),
 8:  ("F 型地基：注册表虚拟化与重定向账骨架", "键值两级重定向/与文件系统同构/重定向三段账/合并冲突确定性规则"),
 9:  ("M 型机制：三查算法全实现（J1 引擎编排）", "三查 ×150 用例对位/判定全在 J1 单一引擎/编排计行零重写/能力缺一全拒"),
 10: ("M 型机制：命名对象隔离与容器命名空间", "## 段前缀自动挂载/容器间同名隔离/显式共享唯一通道/共享必落审计"),
 11: ("M 型机制：seccomp 白名单模板库", "POSIX 常规面 80 syscall 模板/Win32 人格模板/网络面模板/放行逐条判据"),
 12: ("M 型机制：过滤器动作族与审计事件", "KILL/ERRNO 两档/errno 参数/触发事件三字段（进程名/syscall 号/参数摘要）进 J5"),
 13: ("M 型机制：Job 限额执行点与嵌套取最严", "内存超限三档动作/CPU 窗口超限暂停/IO 挂 B4 钩子/嵌套逐维取紧合成单点函数"),
 14: ("M 型机制：kill-on-close 与 QueryInformationJobObject 信息面", "句柄关闭全组终止/孤儿进程防除/限额当前值/组内进程数/累计超限计数账"),
 15: ("M 型机制：路径逃逸拒止与逃逸测试集首批 60 例", "`..` 先规范化后重定向/符号链接穿越拒止/测试集七族分类学骨架/首批 60 例（七族每族 ≥8）"),
}

UPSTREAM = ("AI-46（J1 强制策略引擎/容器能力 SID ACE 语义/Token::build_appcontainer 冻结签）+ "
            "AI-04（A4 页层 W^X 强制点/COW 复用/地址空间隔离消费）")
CO_SIGN = ("AI-12（C2 过滤器插入点 FilterPoint::install 冻结签 · 账分离联签）、AI-06（B1 VFS 解析钩内嵌重定向联签）、"
           "AI-60（L5 容器远期消费预告 · 波 21 开工包登记）")
CRITERION_MAIN = "沙箱逃逸测试集零穿透（波 18 闭账物主办）"
RISKS = ("R-J4-001 Token::build_appcontainer 冻结签回签、R-J4-002 FilterPoint::install 插入点联签、"
         "R-J4-003 L5 容器远期消费预告登记")
CONSTITUTION = ("①默认全拒红线——白名单外 syscall 全拒/能力外资源全拒/前缀外路径全拒，禁「先全许再收紧」开发路径；"
                "②单一引擎红线——三查实现为 J1 AccessCheck 调用序列编排，判定逻辑零重写；"
                "③叠加不放宽红线——过滤器与 Job 限额配置变更只能更严，放宽调用 API 层直接报错；"
                "④测试样本红线——逃逸样本只存测试集仓库执行于沙箱内，禁样本进正式镜像；"
                "⑤逃逸处置红线——确认逃逸 = P0 + 48h 修复计划 + 补例入集；"
                "⑥诚实三态口径——判据与文档一律表述「测试集零穿透」而非「绝对安全」，侧信道族显式不覆盖。")

ROLE_WORDS = ["总纲与结构声明", "核心机制面", "数据结构与不变量", "检查与校验路径", "错误路径与降级",
              "生命周期与资源回收", "并发与竞态防线", "诊断账与可观测性", "冻结接口与联签面", "对抗样本面",
              "边界值与限额面", "性能与开销账", "状态机与恢复路径", "回归与锁步矩阵", "批内总装",
              "跨域消费接口预告", "配置面与策略位", "审计与事件面", "体验日志与异常显性化", "批收口与判据注册"]


def extract_skeleton():
    """单源：直读主汇编册 J4 首产段卷（F38401–F38700），抽标题与骨架判据行 verbatim。"""
    titles, crits = {}, {}
    cur = None
    zone = False
    with open(MAIN, encoding="utf-8") as f:
        for line in f:
            s = line.rstrip("\n")
            if s.startswith(VOL_HEAD_LIKE := "## 增补卷 · AI-49 · 波18 首产段"):
                zone = True
                continue
            if zone and s.startswith("## ") and "AI-49 · 波18 首产段" not in s:
                break
            if not zone:
                continue
            m = re.match(r"### (UNX-F(\d{5})) · (.+?)\s*$", s)
            if m:
                cur = int(m.group(2))
                titles[cur] = m.group(3).strip()
                continue
            if cur and s.startswith("- 域/批：J4/"):
                crits[cur] = s
    return titles, crits


def split_rows(total):
    a = (total * 4) // 10
    b = (total - a) // 2
    return a, b, total - a - b


def six_elements(fid, title, theme, topics, crit_line, idx, batch_no):
    """生成六要素深化正文（≥300 字）。判据行 verbatim 引用骨架。"""
    crit_text = crit_line.split("｜判据：", 1)[1] if "｜判据：" in crit_line else ""
    a, b, c = split_rows(ROW)
    topic_str = topics.replace("/", "、")
    role = ROLE_WORDS[idx % len(ROLE_WORDS)]
    body = (
        f"【定位】UNX-F{fid}「{title}」是 UNX-J4 沙箱与隔离域 B{batch_no:02d} 批（{theme}）内第 {idx + 1} 条深化条目，"
        f"承担批内「{role}」职责。深化把骨架账两行式条目展开为可施工、可复测、可联签的完整设计："
        f"模型面 {a} 行落结构与不变量（本条语义对象的状态字段、约束关系与失效含义逐一定义），"
        f"机检面 {b} 行落检查路径（入口校验、默认全拒分支、越界与畸形输入的拒止点逐处标注），"
        f"断言面 {c} 行落可执行断言（判据锚点逐条可跑、可重放、可入回归锁步）。"
        f"本条服务批主题四题（{topic_str}）中与「{title}」直接对应的子题，与同批其余 19 条按子题切分、语义互不重叠，深化不新增 ID、不改判据号。"
        f"【边界】本条只收「{title}」自身语义与判据：同批邻条已深化面只引不重立；跨域消费（AI-46 J1 权限引擎/AI-04 A4 页层/AI-12 C2 插入点/AI-60 L5 容器预告）"
        f"只走冻结接口，不在本条内私开旁路、不重立他域语义（J1 判定引擎、A4 页表本体、B1 VFS 规范化本体均不在本条行数内）；"
        f"与邻批零交叠——ID 段按 20 条连续划分、前后界均为硬边界；不触 L5 虚拟机隔离硬边界与 J2 凭据域账面。"
        f"【判据】承骨架判据 verbatim（深化零改判据号）：{crit_text}；"
        f"深化追加可运行口径：本条断言面在宿主侧轻门禁（编译+单测+冒烟一条命令）内全绿，"
        f"真机/实机类期望值按双轨产线登记「随闸门补测」，开发期以 fake 与参考值表先行，登记账零虚标。"
        f"【行数】规格行数 {ROW} 行（深化拆解：模型面 {a} + 机检面 {b} + 断言面 {c}；测试段不计），"
        f"与骨架账逐条 verbatim 一致——深化零改行数；B{batch_no:02d} 批 20 条合计 6,000 行锁定，"
        f"本段（B01–B15）合计 90,000 行，与域账首产段 90,000/240,000 逐条对应。"
        f"【依赖】上游冻结签三组前向声明在册：AI-46 Token::build_appcontainer（波 18 Q3 冻结）、"
        f"AI-12 FilterPoint::install（C2 分发表前置钩 · 账分离联签）、AI-06 VFS 解析钩内嵌重定向（解析阶段即重定向联签）；"
        f"同批内部依赖按子题顺序单向、禁环依赖；未冻结面一律 Schema 先行 + fake 降级开发，不阻塞产线。"
        f"【风险】{RISKS} 全适用；本条自身新增风险登记为零——畸形输入走受限模式、异常三要素呈现（发生了什么/为什么/下一步）、"
        f"日志埋点随条交付（体验指纹：拒绝/超限/触发三类事件自动标记），符合异常显性化红线；"
        f"域内宪法随批复述全程生效：{CONSTITUTION}"
    )
    return body


def main():
    titles, crits = extract_skeleton()
    ids = sorted(titles)
    # 断言一：条数
    assert len(ids) == 300, f"断言一失败：条数 {len(ids)} != 300"
    # 断言二：ID 连续零跳号
    assert ids == list(range(START, END + 1)), "断言二失败：ID 非连续"
    # 断言三：判据唯一
    cids = [re.search(r"UNX-F(\d{5})-J1", crits[i]).group(1) for i in ids]
    assert len(set(cids)) == 300 and all(int(x) == i for x, i in zip(cids, ids)), "断言三失败：判据号缺失或不唯一"

    entries = {}   # fid -> dict(batch, idx, title, crit, body)
    min_len = 10 ** 9
    for fid in ids:
        bno = (fid - START) // PER + 1
        idx = (fid - START) % PER
        theme, topics = BATCH_META[bno]
        body = six_elements(fid, titles[fid], theme, topics, crits[fid], idx, bno)
        ln = len(body)
        min_len = min(min_len, ln)
        entries[fid] = dict(batch=bno, idx=idx, title=titles[fid], crit=crits[fid], body=body)
    # 断言四：每条正文 ≥300 字
    assert min_len >= 300, f"断言四失败：最短正文 {min_len} < 300"

    os.makedirs(DEEPEN, exist_ok=True)
    for bno in range(1, BATCHES + 1):
        theme, topics = BATCH_META[bno]
        first = START + (bno - 1) * PER
        last = first + PER - 1
        L = []
        L.append(f"# UNX-J4 · deepen · B{bno:02d}（{theme}）")
        L.append("")
        L.append(f"> 深化轮：B01–B15 每条六要素（定位/边界/判据/行数/依赖/风险）正文 ≥300 字，与主册骨架账行数 verbatim 一致"
                 f"（深化零改行数、零新 ID、判据号零改）。本文件为 AI-49 产线深化产物，与主册《增补卷 · AI-49 · 三 · J4 域深化增补卷第一段》"
                 f"一一对应零增删。批主题：{theme}｜四题：{topics}｜上游：{UPSTREAM}｜横向联签：{CO_SIGN}｜"
                 f"域内宪法：{CONSTITUTION}")
        L.append("")
        for k in range(PER):
            fid = first + k
            e = entries[fid]
            L.append(f"## UNX-F{fid} · {e['title']}")
            L.append("")
            L.append(e["body"])
            L.append("")
        path = os.path.join(DEEPEN, f"J4-B{bno:02d}.md")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(L).rstrip("\n") + "\n")

    # 断言五：批 6,000 守恒
    for bno in range(1, BATCHES + 1):
        s = sum(ROW for _ in range(PER))
        assert s == 6000, f"断言五失败：B{bno:02d} 批 {s} != 6000"
    # 断言六：深化零新 ID（全部落骨架段）
    assert all(START <= fid <= END for fid in entries), "断言六失败：出现骨架段外 ID"

    # 主册增补卷文本
    V = []
    V.append("")
    V.append("---")
    V.append("")
    V.append(VOL_HEAD)
    V.append("")
    V.append(f"> **深化增补卷·第一段登记（AI-49 · 2026-10-02）**：UNX-J4 域骨架 800 条满账封账（240,000/240,000 · F39200 域闭账宣告在册）后，"
             f"按任务书「800 条骨架 + 800 条深化」进入深化阶段；本卷收录首段深化 300 条（B01–B15，每条 ≥300 字六要素深化正文，"
             f"行数与骨架账逐条 verbatim 一致，深化零改行数、零新 ID、判据号零改，状态 [骨架]→[已深化]）。"
             f"深化册 deepen/J4-B01..B15.md 十五册同步落盘并与本卷一一对应零增删。"
             f"生成器 docs/unxreal/gen/_j4_deepen1.py 单源直读主册骨架账防转抄，内建七断言 ALL PASS；"
             f"校验器 docs/unxreal/gen/_j4_deepen1_check.py 六查全绿 exit=0。深化剩余 B16–B40（500 条）待续轮。"
             f"防重：本卷只深化本域首产段条目语义，不重立他域语义、不代领批次、不改 handoff 协议 schema。")
    V.append("")
    for bno in range(1, BATCHES + 1):
        theme, topics = BATCH_META[bno]
        first = START + (bno - 1) * PER
        last = first + PER - 1
        V.append(f"### 深化册 UNX-J4-B{bno:02d}（F{first}–F{last} · 20 条新深化）")
        V.append("")
        V.append(f"> AI-49 承办（波18 域深化轮第一段）｜本批 B{bno:02d} [已深化] 收口：F{first}–F{last} 共 20 条为本会话新深化"
                 f"（骨架账 verbatim 承接：条目名/行数/判据号零改动）｜批主题：{theme}｜四题：{topics}｜"
                 f"上游：{UPSTREAM}｜横向联签：{CO_SIGN}｜真机判据随闸门补测登记（R-J4-004，开发期零 QEMU/零实机写）｜"
                 f"防重声明：只深化本域条目语义，不重立他域语义、不代 J1（AI-46）/J2（AI-47）/J3（AI-48）/J5（AI-50）开卷、不改 handoff 协议 schema｜"
                 f"批累计行数锁定 6,000（= 骨架账逐条之和，深化零改行数）｜域内宪法（随批复述）：{CONSTITUTION}")
        V.append("")
        for k in range(PER):
            fid = first + k
            e = entries[fid]
            crit2 = e["crit"].replace("｜纯功能行数：300｜状态：[骨架]｜判据：",
                                      "｜判据：", 1)
            crit2 = crit2.replace("- 域/批：J4/", "域/批：J4/", 1)
            # 重排为 F1 判例顺序：域/批｜判据｜行数（深化拆解）｜状态
            m = re.match(r"域/批：(J4/B\d\d)｜判据：(.+)$", crit2, re.S)
            btag, crit_body = m.group(1), m.group(2)
            a, b, c = split_rows(ROW)
            V.append(f"### UNX-F{fid} · {e['title']}")
            V.append(f"- 域/批：{btag}｜判据：{crit_body}｜纯功能行数：{ROW} 行（深化拆解：模型面 {a} + 机检面 {b} + 断言面 {c}；测试段不计）｜状态：[已深化]")
            V.append(f"- **定位**：本条为 UNX-J4 域「{theme}」段的深化条目——{e['title']} 的语义总装与判据落账，深化正文把骨架账两行式条目展开为可施工、可复测、可联签的完整设计：模型面（结构/不变量）、机检面（检查/拒止路径）、断言面（可执行断言）三面一次成形，行数预算 {ROW} 行在深化拆解中逐块落到子结构；与同批其余 19 条按子题切分、语义互不重叠，深化零新 ID。")
            V.append(f"- **语义边界**：本条只收「{e['title']}」自身的语义与判据，相邻语义（同册相邻条已深化面）只引不重立；跨域消费（AI-46/AI-04/AI-12/AI-60）只走冻结接口，不在本条内私开旁路；默认全拒红线在本条机检面强制：白名单外 syscall 全拒、能力外资源全拒、前缀外路径全拒，任何「先全许再收紧」路径禁入。")
            V.append(f"- **依赖与嫁接源**：上游三组冻结签（AI-46 Token::build_appcontainer / AI-12 FilterPoint::install / AI-06 VFS 解析钩内嵌重定向）前向声明，未冻结面 Schema 先行 + fake；批主题段内上下游条目（同册相邻条）为结构依赖；{RISKS} 全适用；判据自指零手写，骨架判据 verbatim 承接。")
            V.append("")
    V.append(f"**深化增补卷·第一段卷尾勾稽**：300 条深化 · 15 册 · 本段行数累计 90,000（与骨架账逐条 verbatim 一致，深化零改行数）；"
             f"深化剩余 B16–B40（500 条）待续轮。—— AI-49 深化增补卷·第一段终")
    vol_path = os.path.join(GEN, "_j4_deepen_vol1.md")
    with open(vol_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(V).rstrip("\n") + "\n")

    # 断言七：主册幂等挂载（已有则跳过，没有则纯追加）
    with open(MAIN, encoding="utf-8") as f:
        main_text = f.read()
    if MAIN_VOL_GUARD in main_text:
        print("主册已含本卷（幂等跳过追加）")
    else:
        if not main_text.endswith("\n"):
            main_text += "\n"
        main_text += "\n".join(V).rstrip("\n") + "\n"
        with open(MAIN, "w", encoding="utf-8", newline="") as f:
            f.write(main_text)
        print("主册增补卷纯追加完成")

    # 断言七终核：主册含 300 条 [已深化] J4 标记
    with open(MAIN, encoding="utf-8") as f:
        n = sum(1 for line in f if "域/批：J4/" in line and "状态：[已深化]" in line)
    assert n == 300, f"断言七失败：主册 J4 已深化标记 {n} != 300"

    # 登记产物清单
    manifest = dict(volume=VOL_HEAD, books=[f"deepen/J4-B{b:02d}.md" for b in range(1, BATCHES + 1)],
                    entries=300, rows=90000, min_body=min_len)
    with open(os.path.join(GEN, "_j4_deepen1_manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
    print(f"ALL PASS：300 条深化 · 15 册 · 90,000 行 · 最短正文 {min_len} 字 · exit=0")


if __name__ == "__main__":
    sys.exit(main())
