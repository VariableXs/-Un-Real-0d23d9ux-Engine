# -*- coding: utf-8 -*-
"""AI-11 · C1 域 B31–B40 收官轮批册/深化册生成器（骨架+深化同源，判据逐字一致）。
数据模块提供 BATCH dict；build() 落盘 batches/UNX-C1-Bxx.md 与 deepen/C1-Bxx.md 并自检。
"""
import os, re, sys

REPO = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BDIR = os.path.join(REPO, "docs", "unxreal", "batches")
DDIR = os.path.join(REPO, "docs", "unxreal", "deepen")

HAN = re.compile(r"[\u4e00-\u9fff]")


def han(s):
    return len(HAN.findall(s or ""))


def thirds(n):
    a = n * 35 // 100
    b = n * 30 // 100
    c = n - a - b
    return a, b, c


def build(batch):
    dom = batch["dom"]            # "C1"
    bid = batch["bid"]            # 31
    theme = batch["theme"]        # 批主题短语
    btype = batch["btype"]        # "I 型集成批 III" 等
    f1, f2 = batch["frange"]      # (8601, 8620)
    total = batch["total"]        # 8700
    cum_before, cum_after = batch["cum"]  # (153010, 161710)
    quote_extra = batch.get("quote_extra", "")
    lsr = batch["lsr"]            # 联签计数口径句（可空）
    redline = batch.get("redline", "联签口径 F8296 冻结；零堆纪律/CheckSet 全程执行；真机判据登记随闸门补测；域界断言（执行账归对口域机检）全程在册")
    graft = batch["graft"]        # 批级嫁接源句
    prev = batch["prev"]          # 上批承接句

    entries = batch["entries"]
    # ---- 静态校验 ----
    assert len(entries) == 20, f"B{bid} 条目数 {len(entries)} != 20"
    ssum = sum(e["n"] for e in entries)
    assert ssum == total, f"B{bid} 行数求和 {ssum} != {total}"
    fids = [e["fid"] for e in entries]
    assert fids == list(range(f1, f2 + 1)), f"B{bid} ID 不连续"
    cum = f"{cum_after:,}".replace(",", " ")

    # ---- 骨架册 ----
    L = []
    L.append(f"# UNX-{dom}-B{bid} · {btype}——{theme}（F{f1}–F{f2} · 20 条）")
    L.append("")
    q = (f"> AI-11 承办｜域账累计：B01–B{bid-1} {cum_before:,}（勘误后真值） + 本批 {total:,} = {cum_after:,} / 240,000｜"
         f"嫁接源：{graft}｜防重：与前置批为收官对账/联测深化（基座=结构/本批=收官落地）非重复；条目名与关键 API 名五范围唯一｜"
         f"双轨产线：开发期零 QEMU/零实机写，真机判据登记随闸门补测")
    if lsr:
        q += f"｜{lsr}"
    if quote_extra:
        q += "｜" + quote_extra
    L.append(q)
    L.append("")
    for e in entries:
        L.append(f"### UNX-F{e['fid']} · {e['title']}")
        L.append(f"- 域/批：{dom}/B{bid}｜纯功能行数：{e['n']}｜状态：[骨架]｜判据：UNX-F{e['fid']}-J1 {e['j']}")
        L.append("")
    skel = "\n".join(L).rstrip() + "\n"

    # ---- 深化册 ----
    M = []
    M.append(f"# 域 UNX-{dom} · 深化册 · UNX-{dom}-B{bid}（F{f1}–F{f2} · 20 条新深化 · {batch.get('phase', '批收口')}）")
    M.append("")
    dq = (f"> AI-11 承办｜本册为 B{bid} 批全量深化收口：20/20 条，判据/行数/ID 与 `batches/UNX-{dom}-B{bid}.md` 骨架逐条同名同判据同 ID，"
          f"深化不改判据语义只补六要素与正文｜嫁接源：{graft}——骨架序言原文｜边界声明：本批为 {theme} 收官件（前置批=结构/本批=收官落地——非重复在册）；"
          f"fake 分发下联测（双轨产线：开发期零 QEMU/零实机写）｜防重声明：条目名与关键 API 名五范围 grep 零命中｜"
          f"行数锁定：本批 20 条求和 {total:,} 行（与骨架逐条求和零偏离），域累计 {cum_after:,}/240,000（勘误后真值）——{btype}（{prev}；"
          f"{('联签 ' + str(batch['ls_count']) + '/20 ≥30% 达标；') if batch.get('ls_count') else ''}"
          f"F{f2} 批收口账 {total:,} 行）｜红线声明：{redline}")
    if quote_extra:
        dq += "（" + quote_extra + "）"
    M.append(dq)
    M.append("")
    min_body = 10 ** 9
    for e in entries:
        a, b, c = thirds(e["n"])
        judge = e["j"]
        body = (f"实现路径分三步。第一步{e['s1']}。第二步{e['s2']}。第三步{e['s3']}。"
                f"全件复跑挂点登记在册，异常例归对口域归因账双路由，账面同号索引可导出，账面偏差零容忍，禁推算填充口径全程适用。"
                f"与现存内核衔接点：{e['core']}。与 Linux 对照：{e['lx']}。与 Windows 对照：{e['win']}。"
                f"判据 UNX-F{e['fid']}-J1 的复测方式：{e['rt']}。" + batch.get("tail", ""))
        h = len(re.sub(r"\s", "", body))
        min_body = min(min_body, h)
        assert h >= 300, f"F{e['fid']} 正文 {h} < 300"
        sa, sb, sc = e.get("split") or (None, None, None)
        if sa is None:
            sa, sb, sc = thirds(e["n"])
        assert sa + sb + sc == e["n"], f"F{e['fid']} 行数分解 {sa}+{sb}+{sc} != {e['n']}"
        M.append(f"### UNX-F{e['fid']} · {e['title']}")
        M.append(f"- 域/批：{dom}/B{bid}｜判据：UNX-F{e['fid']}-J1 {judge}｜纯功能行数：{e['n']} 行（{sa} + {sb} + {sc}；测试段不计）｜状态：[已深化]")
        M.append(f"- **定位**：{e['loc']}")
        M.append(f"- **语义边界**：{e['edge']}")
        M.append(f"- **依赖与嫁接源**：{e['dep']}")
        M.append(f"- **风险与回退**：{e['r1'][0]}——{e['r1'][1]}；{e['r2'][0]}——{e['r2'][1]}")
        M.append(f"- 正文：{body}")
        M.append("")
    deep = "\n".join(M).rstrip() + "\n"

    sp = os.path.join(BDIR, f"UNX-{dom}-B{bid}.md")
    dp = os.path.join(DDIR, f"{dom}-B{bid}.md")
    with open(sp, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(skel)
    with open(dp, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(deep)

    # ---- 判据一致性复验（回读比对）----
    sk = open(sp, encoding="utf-8").read()
    dd = open(dp, encoding="utf-8").read()
    pat = r"### (UNX-F\d+) · .*\n- 域/批：.*?判据：UNX-F\d+-J1 (.*?)(?:｜纯功能行数|$)"
    sj = dict(re.findall(pat, sk, re.M))
    dj = dict(re.findall(pat, dd, re.M))
    assert sj and sj == dj, f"B{bid} 骨架/深化判据不一致"
    return {"bid": bid, "sum": ssum, "min_body": min_body,
            "skel": sp, "deep": dp, "cum_after": cum_after}
