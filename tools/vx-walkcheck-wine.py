#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""vx-walkcheck-wine.py — 交叉走查第一项证据回收（Windows 域侧）

用途：Variable 在 VARIX 域完成六步走查、回到 Windows 后运行本脚本。
流程：探测快照卷 → 收集 PPM 截图 / wineserver 日志 / 前缀清单 →
      逐判据核对（B-1001/1002/1003/评级/窗口/清算）→ 生成证据包
      JSON + Markdown 报告 → 打印台账登记行（人工粘贴进工作包台账）。

纪律：只读采集，零系统写入（除自身输出目录）；零第三方依赖。
用法：python tools/vx-walkcheck-wine.py [--src V:\\walkcheck] [--out docs/acceptance]
"""
import argparse
import hashlib
import json
import os
import re
import sys
from datetime import date

JUDGE_KEYS = ["B-1001", "B-1002", "B-1003", "rating", "window", "teardown"]

MARKERS = {
    # wineserver 惰性拉起 + 看门狗重建（B-1001）
    "B-1001": [r"wineserver.*(spawn|lazy|拉起)", r"rebuild.*(ok|done|恢复)", r"watchdog.*alive"],
    # 前缀实例化带模板版本（B-1002）
    "B-1002": [r"prefix.*ver(=|:)\s*[1-9]", r"template[_-]ver\s*[1-9]"],
    # 零侵入常量复读（B-1003）
    "B-1003": [r"WINE_SOURCE_DIFF_LINES\s*=\s*0", r"diff.*0.*zero-intrusion"],
    # 评级如实（partial 不虚标）
    "rating": [r"notepad-classic.*partial", r"tier.*partial"],
    # 会话清算
    "teardown": [r"session.*(clean|清算|teardown)", r"exit.*0.*(residue|残留).*0"],
}


def find_candidate_volumes():
    """按标签/特征探测候选卷：SNAPSHOT、VARIX_SYS，以及手动 --src。"""
    cands = []
    for disk in "DEFGHIJKLMNOPQRSTUVW":
        root = f"{disk}:\\"
        if not os.path.isdir(root):
            continue
        try:
            vol = os.path.abspath(root)
            label = win_label(disk)
        except OSError:
            continue
        if label and any(k in label.upper() for k in ("SNAPSHOT", "VARIX")):
            cands.append((root, label))
    return cands


def win_label(disk):
    try:
        import ctypes
        buf = ctypes.create_unicode_buffer(261)
        ctypes.windll.kernel32.GetVolumeInformationW(
            f"{disk}:\\", buf, 260, None, None, None, None, 0)
        return buf.value or ""
    except Exception:
        return ""


def walk_collect(src, limit=200):
    """收集证据文件：ppm 截图、log/txt 日志、前缀目录线索。"""
    shots, logs, hits = [], [], []
    for dirpath, dirnames, filenames in os.walk(src):
        dirnames[:] = [d for d in dirnames if d.lower() not in ("system volume information", "$recycle.bin")]
        for fn in filenames:
            p = os.path.join(dirpath, fn)
            low = fn.lower()
            rel = os.path.relpath(p, src)
            if low.endswith((".ppm", ".png", ".bmp")):
                shots.append((rel, p, os.path.getsize(p)))
            elif low.endswith((".log", ".txt", ".json")):
                logs.append((rel, p, os.path.getsize(p)))
            if "wine" in rel.lower() and os.path.isfile(p):
                hits.append((rel, p))
            if len(shots) + len(logs) > limit * 3:
                break
    return shots, logs, hits


def match_evidence(logs, hits):
    """逐判据匹配标记行。"""
    found = {k: [] for k in JUDGE_KEYS}
    blobs = []
    for rel, p, _ in logs:
        try:
            if os.path.getsize(p) < 2_000_000:
                blobs.append((rel, open(p, "r", errors="ignore").read()))
        except OSError:
            pass
    for rel, p in hits:
        try:
            if os.path.getsize(p) < 2_000_000:
                blobs.append((rel, open(p, "r", errors="ignore").read()))
        except OSError:
            pass
    for key, pats in MARKERS.items():
        for rel, text in blobs:
            for pat in pats:
                m = re.search(pat, text, re.IGNORECASE)
                if m:
                    line = text[:m.end()].splitlines()[-1][:160]
                    found[key].append(f"{rel} :: {line}")
                    break
    return found


def sha256(p):
    h = hashlib.sha256()
    try:
        with open(p, "rb") as f:
            for chunk in iter(lambda: f.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()[:16]
    except OSError:
        return "unreadable"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", help="手动指定证据源目录（默认自动探测快照卷）")
    ap.add_argument("--out", default="docs/acceptance")
    a = ap.parse_args()

    srcs = [(a.src, "manual")] if a.src else find_candidate_volumes()
    if not srcs:
        print("[X] 未找到候选卷（含 SNAPSHOT/VARIX 标签）。插好 U 盘后重试，或 --src 手动指定。")
        return 2
    print("[i] 候选证据源:", srcs)

    all_shots, all_logs, all_hits = [], [], []
    for root, label in srcs:
        s, l, h = walk_collect(root)
        print(f"[i] {root} ({label}): 截图 {len(s)} / 日志 {len(l)} / wine 相关 {len(h)}")
        all_shots += s; all_logs += l; all_hits += h

    found = match_evidence(all_logs, all_hits)
    greens = [k for k in JUDGE_KEYS if found[k]]
    reds = [k for k in JUDGE_KEYS if not found[k]]

    outdir = os.path.join(a.out, f"{date.today()}-交叉走查第一项-VARIX域内Windows应用")
    os.makedirs(outdir, exist_ok=True)

    evidence = {
        "date": str(date.today()),
        "item": "交叉走查第一项 · VARIX 域内 Windows 应用（notepad-classic）",
        "source": [r for r, _ in srcs],
        "screenshots": [{"rel": r, "size": s, "sha256_16": sha256(p)} for r, p, s in all_shots[:30]],
        "judgements": {k: {"verdict": "GREEN" if k in greens else "RED",
                           "evidence": found[k][:3]} for k in JUDGE_KEYS},
        "summary": f"GREEN {len(greens)}/{len(JUDGE_KEYS)}；RED 项：{reds or '无'}",
    }
    jp = os.path.join(outdir, "证据包.json")
    with open(jp, "w", encoding="utf-8") as f:
        json.dump(evidence, f, ensure_ascii=False, indent=2)

    mp = os.path.join(outdir, "报告.md")
    with open(mp, "w", encoding="utf-8") as f:
        f.write(f"# 交叉走查第一项 · 结果报告（{date.today()}）\n\n")
        f.write(f"证据源：{[r for r, _ in srcs]}\n\n")
        f.write(f"截图：{len(all_shots)} 张 ｜ 日志：{len(all_logs)} 份\n\n")
        f.write("| 判据 | 判定 | 证据 |\n| --- | --- | --- |\n")
        for k in JUDGE_KEYS:
            v = "✅ 绿" if k in greens else "❌ 红"
            e = (found[k][0] if found[k] else "未找到标记行").replace("|", "/")
            f.write(f"| {k} | {v} | {e} |\n")
        f.write(f"\n**小结：{evidence['summary']}**\n\n纪律：失败也是走查——红项如实登记欠账，不现场修。\n")

    print("\n=== 判定 ===")
    for k in JUDGE_KEYS:
        print(f"  {k}: {'GREEN' if k in greens else 'RED'}", found[k][:1])
    print(f"\n[OK] 证据包: {jp}\n[OK] 报告: {mp}")
    print("\n[台账登记行]（粘贴进 工作包台账.md 的 m3 段）:")
    print(f"  - 交叉走查第一项（VARIX 域内 Windows 应用）{date.today()}: 判据 {len(greens)}/6 绿"
          f"{'，红项 ' + ','.join(reds) if reds else '，全绿'}——证据见 {outdir}")
    return 0 if not reds else 1


if __name__ == "__main__":
    sys.exit(main())
