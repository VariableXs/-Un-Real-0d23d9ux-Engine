# -*- coding: utf-8 -*-
"""CoRun Varix STAR II · Unxreal — batches 批册全量汇编生成器"""
import os, re, sys, datetime, collections

REPO  = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
BATCH = os.path.join(REPO, "docs", "unxreal", "batches")
OUT   = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")

PARTS = [
    ("部A", "引导与内核地基", "知识层一：引导/中断/调度/内存/设施", ["A1", "A2", "A3", "A4", "A5"]),
    ("部B", "文件与存储栈", "知识层二：文件/NTFS/块/RAID", ["B1", "B2", "B3", "B4", "B5"]),
    ("部C", "人格与用户态生态", "知识层三：人格/系统调用/POSIX/IPC/生态", ["C1", "C2", "C3", "C4", "C5"]),
    ("部D", "NT 语义域", "知识层四：NT 语义四域（本册现收 D1–D3）", ["D1", "D2", "D3"]),
]
DOMAINS = {
    "A1": ("引导与早年初始化", "AI-01"), "A2": ("中断与计时基座", "AI-02"), "A3": ("SMP 与调度", "AI-03"),
    "A4": ("内存管理成熟体", "AI-04"), "A5": ("内核基础设施", "AI-05"), "B1": ("VFS 与路径层", "AI-06"),
    "B2": ("ext 与只读 fs 生态", "AI-07"), "B3": ("NTFS 与 Windows 卷语义", "AI-08"), "B4": ("块层与设备管理", "AI-09"),
    "B5": ("RAID/LVM/加密与数据安全", "AI-10"), "C1": ("ELF 装载与进程模型", "AI-11"), "C2": ("系统调用面域", "AI-12"),
    "C3": ("POSIX 认证与 LTP 验收", "AI-13"), "C4": ("IPC 与 Unix 语义", "AI-14"), "C5": ("用户态生态嫁接", "AI-15"),
    "D1": ("NT API 语义面（ntdll）", "AI-16"), "D2": ("PE 装载与进程线程语义", "AI-17"), "D3": ("注册表与 NT 对象命名空间", "AI-18"),
}
DOMAIN_NOTES = {
    "A1": "B01–B02 两批批册不在 batches/（深化册在 deepen/A1-B01、A1-B02，样板批次另见总纲 §5）——本册收录 B03–B40。",
    "A5": "B16 起批册为骨架单册体例（批内条目清单 + 批级守恒预核同册）。",
    "B3": "本域无独立批册——深化册即批册（单册体例），800 条见 deepen/B3-B01..B40，域满账封账 240,000 行；本汇编批面暂缺，待统一体例后增补。",
}
H1_A = re.compile(r"^#\s+(UNX-[A-P]\d-B\d+)\s*·\s*(.*)$")
FRANGE = re.compile(r"（F(\d+)[–\-—]F?(\d+)\s*(?:·\s*(\d+)\s*条)?）\s*$")
H1_B = re.compile(r"^#\s*UNX-([A-P]\d)\s*·\s*B(\d+)(?:\s*骨架)?\s*（F(\d+)[–\-—]F?(\d+)）\s*·\s*(.*)$")
HEAD_A = re.compile(r"^###\s+(UNX-F\d+)\s*·\s*(.+?)\s*$")
HEAD_B = re.compile(r"^###\s+(F\d+)\s+(.+?)\s*$")
META_A = re.compile(r"^- 域/批：.*?纯功能行数：(\d+)｜状态：\[?(.*?)\]?｜判据：(.*)$")
META_B = re.compile(r"^- 域 UNX-[A-P]\d\s*·\s*批 B\d+\s*·\s*行数锁定\s*(\d+)\s*·\s*状态\s*\[(.+?)\]\s*$")
JLINE = re.compile(r"^- 判据\s*(J\d+)：(.*)$")


def esc(s):
    return s.replace("|", "\\|").replace("\n", " ").strip()


def parse_file(path, fn):
    with open(path, encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    rec = {"file": fn, "batch": None, "title": "", "f1": None, "f2": None, "n_claim": None,
           "quotes": [], "entries": [], "tail": [], "warn": []}
    i, n = 0, len(lines)
    while i < n and not lines[i].startswith("# "):
        if lines[i].strip():
            rec["warn"].append("no-H1-yet:" + lines[i][:40])
            return rec
        i += 1
    h1 = lines[i].strip()
    i += 1
    m = H1_A.match(h1)
    if m:
        rec["batch"], rest = m.group(1), m.group(2)
        fm = FRANGE.search(rest)
        if fm:
            rec["f1"], rec["f2"], rec["n_claim"] = int(fm.group(1)), int(fm.group(2)), fm.group(3)
            rest = rest[:fm.start()].strip()
        rec["title"] = rest
    else:
        m = H1_B.match(h1)
        if not m:
            rec["warn"].append("H1-unparsed:" + h1[:80])
            return rec
        rec["batch"] = "UNX-%s-B%02d" % (m.group(1), int(m.group(2)))
        rec["f1"], rec["f2"] = int(m.group(3)), int(m.group(4))
        rec["title"] = m.group(5).strip()
    while i < n and lines[i].strip() == "":
        i += 1
    while i < n and lines[i].startswith(">"):
        rec["quotes"].append(lines[i][1:].strip())
        i += 1
    cur = None
    tail_mode = False
    while i < n:
        s = lines[i].strip()
        if s == "" or s == "---":
            i += 1
            continue
        if s.startswith("### "):
            mh = HEAD_A.match(s) or HEAD_B.match(s)
            if mh:
                cur = {"id": mh.group(1), "title": (mh.group(2) or "").strip(), "rows": None,
                       "status": "", "judge": []}
                rec["entries"].append(cur)
                tail_mode = False
                i += 1
                continue
            # 非条目型 H3（如批内小标题）：保留为留痕
            body = s[4:].strip()
            if cur is not None and not tail_mode:
                cur["judge"].append("[小节] " + body)
            else:
                rec["tail"].append("[小节] " + body)
            i += 1
            continue
        if s.startswith("## "):
            sec = s[3:].strip()
            if sec == "批内条目清单":
                tail_mode = False
            else:
                tail_mode = True
                rec["tail"].append(sec)
            i += 1
            continue
        if s.startswith("# "):
            rec["warn"].append("stray-H1:" + s[:50])
            i += 1
            continue
        if s.startswith(">"):
            # 引用行（批级/条目级留痕，散落于条目之间）——一律保真保留
            body = s[1:].strip()
            if cur is not None and not tail_mode:
                cur["judge"].append("[留痕] " + body)
            else:
                rec["tail"].append(body)
            i += 1
            continue
        if cur is not None and not tail_mode:
            ma = META_A.match(s)
            mb = META_B.match(s)
            mj = JLINE.match(s)
            if ma:
                cur["rows"], cur["status"], cur["judge"] = int(ma.group(1)), ma.group(2).strip(), [ma.group(3).strip()]
            elif mb:
                cur["rows"], cur["status"] = int(mb.group(1)), mb.group(2).strip()
            elif mj:
                cur["judge"].append(mj.group(1) + "：" + mj.group(2).strip())
            else:
                rec["warn"].append("loose-line:" + s[:70])
            i += 1
            continue
        if tail_mode and s:
            rec["tail"].append(s)
        elif cur is None and not tail_mode and s:
            rec["warn"].append("pre-entry-line:" + s[:60])
        i += 1
    if rec["n_claim"] is None and rec["entries"]:
        rec["n_claim"] = str(len(rec["entries"]))
    return rec


def main():
    files = sorted(f for f in os.listdir(BATCH) if f.startswith("UNX-") and f.endswith(".md"))
    recs, warnings = [], []
    for fn in files:
        r = parse_file(os.path.join(BATCH, fn), fn)
        recs.append(r)
        warnings += [fn + ": " + w for w in r["warn"]]
        if len(r["entries"]) != 20:
            warnings.append(fn + ": entry-count=" + str(len(r["entries"])))
        if r["entries"]:
            nums = sorted(int(e["id"].replace("UNX-F", "").replace("F", "")) for e in r["entries"])
            if r["f1"] is not None and (nums[0] != r["f1"] or nums[-1] != r["f2"]):
                warnings.append("%s: frange-mismatch H1=(%s-%s) actual=(%s-%s)" % (fn, r["f1"], r["f2"], nums[0], nums[-1]))

    dom = collections.OrderedDict()
    for r in recs:
        d = r["batch"][4:6]
        dd = dom.setdefault(d, {"recs": [], "entries": 0, "rows": 0, "deep": 0, "skel": 0,
                                "fmin": None, "fmax": None, "ais": set(), "batches": []})
        dd["recs"].append(r)
        dd["batches"].append(r["batch"])
        dd["entries"] += len(r["entries"])
        for e in r["entries"]:
            dd["rows"] += e["rows"] or 0
            if "深化" in e["status"]:
                dd["deep"] += 1
            elif "骨架" in e["status"]:
                dd["skel"] += 1
            num = int(e["id"].replace("UNX-F", "").replace("F", ""))
            dd["fmin"] = num if dd["fmin"] is None else min(dd["fmin"], num)
            dd["fmax"] = num if dd["fmax"] is None else max(dd["fmax"], num)
        for q in r["quotes"][:1]:
            m = re.search(r"AI-(\d+)", q)
            if m:
                dd["ais"].add("AI-" + m.group(1))

    seen = {}
    for r in recs:
        for e in r["entries"]:
            seen[e["id"]] = seen.get(e["id"], 0) + 1
    dups = [k for k, v in seen.items() if v > 1]
    if dups:
        warnings.append("DUPLICATE-IDS: " + str(dups[:10]) + " (" + str(len(dups)) + ")")

    stamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    total_entries = sum(d["entries"] for d in dom.values())
    total_rows = sum(d["rows"] for d in dom.values())
    total_deep = sum(d["deep"] for d in dom.values())
    total_skel = sum(d["skel"] for d in dom.values())

    L = []
    L.append("# CoRun Varix STAR II · Unxreal · 全量功能总汇编")
    L.append("")
    L.append("> **本册定位**（Variable 明令 2026-09-29）：文件名《CoRun Varix STAR II · Unxreal》专属本册——Varix 计划 **64,000 项功能**的总汇编入口。工程公理 64,000 条 = 80 域 × 40 批 × 20 条；本册随批册产出持续增补，直至 80 域满编。")
    L.append("> ")
    L.append("> **快照口径**：本版为 `docs/unxreal/batches/` 的只读汇编快照（生成时点 %s）——收录 **%d 册批册 / %d 条功能 / %s 行功能行数锁定**（已深化 %d 条 / 骨架 %d 条）。批次状态的唯一权威仍是总纲 §7.3 台账（ADR-06）与 batches/ 源册，深化正文在 deepen/ 深化册；本册只做全 AI 可读的汇编视图，零改写、零裁定、零状态变更。" % (stamp, len(recs), total_entries, "{:,}".format(total_rows), total_deep, total_skel))
    L.append("> ")
    L.append("> **体例说明**：部（知识层）→ 域（UNX-XX）→ 批（UNX-XX-B##）→ 条目表（编号/功能/行数/状态/判据）。表内判据为各条目的可运行验收判据（J1…，含机械用例号 M-###）；「已深化」= 深化册已收口，「骨架」= 批册已立待深化。")
    L.append("")
    L.append("## 卷首 · 快照总览")
    L.append("")
    L.append("| 域 | 域名 | 承办 | 批册 | 条目 | F-ID 区间 | 行数合计 | 已深化 / 骨架 |")
    L.append("|---|---|---|---|---|---|---|---|")
    for pkey, pname, plyr, dlist in PARTS:
        for d in dlist:
            if d not in dom:
                if d == "B3":
                    L.append("| UNX-%s | %s | %s | 0（深化册即批册） | 0（域账 800 条见 deepen/） | F5601–F6400（见 deepen） | 240,000（封账） | — |" % (d, DOMAINS[d][0], DOMAINS[d][1]))
                continue
            dd = dom[d]
            bs = sorted(dd["batches"])
            bspan = "B" + bs[0][-2:] + "–B" + bs[-1][-2:] if len(bs) > 1 else bs[0][-2:]
            L.append("| [UNX-%s](#dom-%s) | %s | %s | %d（%s） | %d | F%04d–F%04d | %s | %d / %d |" % (
                d, d, DOMAINS[d][0], "、".join(sorted(dd["ais"])) or DOMAINS[d][1],
                len(dd["recs"]), bspan, dd["entries"], dd["fmin"], dd["fmax"], "{:,}".format(dd["rows"]), dd["deep"], dd["skel"]))
    L.append("| **合计（17 域）** | — | — | **%d** | **%d** | — | **%s** | **%d / %d** |" % (len(recs), total_entries, "{:,}".format(total_rows), total_deep, total_skel))
    L.append("")
    L.append("**快照缺口与在途如实登记**：")
    L.append("")
    for d, dn in DOMAIN_NOTES.items():
        if d == "B3" or d in dom:
            L.append("- UNX-%s：%s" % (d, dn))
    L.append("- 工程公理口径：80 域 × 800 条 = 64,000 条；本快照覆盖 17 域 %d 条，占公理总量 %.1f%%。其余 63 域批册未立，随认领与产出增补。" % (total_entries, total_entries / 64000.0 * 100))
    L.append("- 并行工况声明：batches/ 为多会话并行产线，本快照生成后新落册批册将在下一版增补（以本页生成时点为准）。")
    L.append("")
    L.append("## 汇编地图")
    L.append("")
    for pkey, pname, plyr, dlist in PARTS:
        got = [d for d in dlist if d in dom or d == "B3"]
        L.append("- **%s · %s**（%s）：%s" % (pkey, pname, plyr,
            " ｜ ".join("[UNX-%s %s](#dom-%s)" % (d, DOMAINS[d][0], d) for d in got)))
    L.append("")
    L.append("---")
    L.append("")

    for pkey, pname, plyr, dlist in PARTS:
        L.append('<a id="part-%s"></a>' % pkey[1])
        L.append("")
        L.append("## %s · %s" % (pkey, pname))
        L.append("")
        L.append("> %s" % plyr)
        L.append("")
        for d in dlist:
            dd = dom.get(d)
            name, ai = DOMAINS[d]
            L.append('<a id="dom-%s"></a>' % d)
            L.append("")
            if dd is None:
                L.append("### UNX-%s · %s" % (d, name))
                L.append("")
                L.append("> 域档｜承办 %s｜批册 0（%s）" % (ai, DOMAIN_NOTES.get(d, "本域批册未立")))
                L.append("")
                continue
            bs = sorted(dd["batches"])
            L.append("### UNX-%s · %s" % (d, name))
            L.append("")
            L.append("> 域档｜承办 %s｜批册 %d（%s–%s）｜条目 %d｜F%04d–F%04d｜行数合计 %s｜已深化 %d / 骨架 %d" % (
                "、".join(sorted(dd["ais"])) or ai, len(dd["recs"]), bs[0][-2:], bs[-1][-2:],
                dd["entries"], dd["fmin"], dd["fmax"], "{:,}".format(dd["rows"]), dd["deep"], dd["skel"]))
            if d in DOMAIN_NOTES:
                L.append("> 域注：%s" % DOMAIN_NOTES[d])
            L.append("")
            for r in sorted(dd["recs"], key=lambda x: int(x["batch"][-2:])):
                L.append("#### %s · %s（F%s–F%s · %s 条）" % (
                    r["batch"], r["title"], str(r["f1"] or ""), str(r["f2"] or ""),
                    r["n_claim"] or str(len(r["entries"]))))
                L.append("")
                for q in r["quotes"]:
                    if q:
                        L.append("> " + esc(q))
                    else:
                        L.append(">")
                if r["tail"]:
                    L.append("> 批级留痕｜" + esc(" ".join(r["tail"]))[:600])
                L.append("")
                L.append("| 编号 | 功能条目 | 行数 | 状态 | 判据 |")
                L.append("|---|---|---|---|---|")
                for e in r["entries"]:
                    j = "<br>".join(esc(x) for x in e["judge"]) if e["judge"] else "—"
                    L.append("| UNX-%s | %s | %s | %s | %s |" % (
                        e["id"].replace("UNX-", ""), esc(e["title"]),
                        e["rows"] if e["rows"] is not None else "—",
                        esc(e["status"].replace("[", "").replace("]", "")) or "—", j))
                L.append("")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(L) + "\n")

    print("=== GENERATION REPORT ===")
    print("files: %d | entries: %d | rows: %s | deep: %d | skel: %d | dups: %d" % (
        len(recs), total_entries, "{:,}".format(total_rows), total_deep, total_skel, len(dups)))
    print("out: %s (%s bytes)" % (OUT, "{:,}".format(os.path.getsize(OUT))))
    print("--- per-domain ---")
    for d in sorted(dom):
        dd = dom[d]
        print("  %s: %d books, %d entries, F%s-F%s, rows %s, deep/skel %d/%d, ai=%s" % (
            d, len(dd["recs"]), dd["entries"], dd["fmin"], dd["fmax"], "{:,}".format(dd["rows"]),
            dd["deep"], dd["skel"], sorted(dd["ais"])))
    print("--- warnings (%d) ---" % len(warnings))
    for w in warnings[:40]:
        print("  ! " + w)
    if len(warnings) > 40:
        print("  ... +%d more" % (len(warnings) - 40))


if __name__ == "__main__":
    main()
