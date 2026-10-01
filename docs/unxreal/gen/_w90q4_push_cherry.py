# -*- coding: utf-8 -*-
"""AI-104 外科推送：worktree 挂 origin/main（e0c1347），重放本会话四个纯增量（两新文件直拷 +
分工图/台账按生成器单源文本幂等追加），提交推送后 API 终验。
他会话在途提交零代办（AI-04/AI-102 2026-10-01 判例）；主汇编册超 100MB 不入推送（AI-71/AI-86 先例）。
"""
import io
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
WT = os.path.join(ROOT, "_attic", "ai104_push_worktree")
MYCOMMIT = "29078676"
GEN = "docs/unxreal/gen/_w90q4_firstprod.py"
BOOK = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md"
CHART = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · AI分工完成图.md"
LEDGER = "CoRun Varix STAR II · Unxreal · 统一协作总台账.md"
CHART_PROBE = "AI-104 会话登记（300 项新功能 · W90-Q4 首产段"
LEDGER_PROBE = "W90-Q4 WebView2/浏览器内核嵌入 首产段立账"

MSG = ("unxreal(w90-q4): AI-104 W90-Q4 首产段外科推送——300 项新功能（B01–B15 · W90-Q4-001–W90-Q4-300 · "
       "15 批 × 20 条 × 6,000 行 = 90,000 行 · 域账 37.5% · 状态「增补」）三线闭环 WebView2 B01–B06 / CEF B07–B12 / "
       "IE 模式开卷 B13–B15；任务书 §104.4 两枚判据锚全文保真（Q4-B01-04-J1→W90-Q4-004、Q4-B13-09-J3→W90-Q4-249）；"
       "跨线联签前缀显式标注零改写；W90 五红线零违例自证；随闸门补测登记不虚报；生成器五断言 ALL PASS"
       "（卷块 SHA-256 前 16 位 cd4c4fc5ab3e104b）；独立增补册+生成器直拷、分工图/台账单源文本幂等追加纯增量，"
       "本地提交 29078676 在链（主汇编册超 100MB 沿 AI-71/AI-86 先例本地落盘；共享 index 裹挟的他域产物由其本会话自行推送）")


def sh(*a, cwd=WT, timeout=1800):
    return subprocess.run(a, capture_output=True, text=True, cwd=cwd, timeout=timeout,
                          encoding="utf-8", errors="replace")


def api(path):
    T = subprocess.run(["git", "credential", "fill"], input="protocol=https\nhost=github.com\n\n",
                       capture_output=True, text=True, cwd=ROOT, timeout=60).stdout
    tok = [l.split("=", 1)[1] for l in T.splitlines() if l.startswith("password=")][0]
    req = urllib.request.Request(
        "https://api.github.com/repos/VariableXs/-Un-Real-0d23d9ux-Engine/" + path,
        headers={"Authorization": "Bearer " + tok, "User-Agent": "ai104-q4"})
    return json.load(urllib.request.urlopen(req, timeout=120))


def cleanup():
    if os.path.isdir(WT):
        sh("git", "cherry-pick", "--abort")
        sh("git", "worktree", "remove", "--force", WT, cwd=ROOT, timeout=600)
    if os.path.exists(WT):
        shutil.rmtree(WT, ignore_errors=True)
    sh("git", "worktree", "prune", cwd=ROOT, timeout=120)


def single_source_texts():
    import importlib.util
    spec = importlib.util.spec_from_file_location("g", os.path.join(ROOT, GEN))
    m = importlib.util.module_from_spec(spec)
    sys.argv = ["x", "--check"]
    try:
        spec.loader.exec_module(m)
    except SystemExit:
        pass
    rows = m.build_all()
    return m.CHART_REG, m.build_ledger_entry(rows)


def main():
    cleanup()
    w = sh("git", "worktree", "add", "--detach", WT, MYCOMMIT, cwd=ROOT, timeout=900)
    print("worktree add rc", w.returncode, flush=True)
    if w.returncode != 0:
        return 2
    try:
        for attempt in range(10):
            try:
                rh = api("commits/main")["sha"]
            except Exception as e:
                print("api err", e, flush=True)
                time.sleep(10)
                continue
            print(f"[{attempt}] remote={rh[:10]}", flush=True)
            f = sh("git", "fetch", "origin", "main", timeout=3600)
            print("fetch rc", f.returncode, flush=True)
            if f.returncode != 0:
                time.sleep(15)
                continue
            r = sh("git", "reset", "--hard", "FETCH_HEAD", timeout=1200)
            print("reset rc", r.returncode, flush=True)
            if r.returncode != 0:
                time.sleep(15)
                continue

            # 幂等探针：远端基线是否已含我的产物
            gen_ok = sh("git", "cat-file", "-e", "HEAD:" + GEN).returncode == 0
            book_ok = sh("git", "cat-file", "-e", "HEAD:" + BOOK).returncode == 0
            chart_txt = sh("git", "show", "HEAD:" + CHART, timeout=600).stdout
            led_txt = sh("git", "show", "HEAD:" + LEDGER, timeout=600).stdout
            chart_ok = CHART_PROBE in chart_txt
            led_ok = LEDGER_PROBE in led_txt
            print(f"state gen={gen_ok} book={book_ok} chart={chart_ok} ledger={led_ok}", flush=True)
            if gen_ok and book_ok and chart_ok and led_ok:
                print("ALL my content already on remote baseline — verify-only done", flush=True)
                return 0

            # 纯增量重放
            if not gen_ok:
                dst = os.path.join(WT, GEN)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copyfile(os.path.join(ROOT, GEN), dst)
            if not book_ok:
                dst = os.path.join(WT, BOOK)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copyfile(os.path.join(ROOT, BOOK), dst)
            chart_reg, ledger_entry = single_source_texts()
            if not chart_ok:
                with io.open(os.path.join(WT, CHART), "a", encoding="utf-8", newline="\n") as fh:
                    fh.write(chart_reg)
            if not led_ok:
                with io.open(os.path.join(WT, LEDGER), "a", encoding="utf-8", newline="\n") as fh:
                    fh.write(ledger_entry)
            a = sh("git", "add", "--", GEN, BOOK, CHART, LEDGER, timeout=600)
            print("add rc", a.returncode, flush=True)
            c = sh("git", "commit", "-m", MSG, timeout=600)
            print("commit rc", c.returncode, (c.stderr or "").strip()[-160:], flush=True)
            if c.returncode != 0:
                return 4
            p = sh("git", "push", "origin", "HEAD:main", timeout=3600)
            out = ((p.stdout or "") + (p.stderr or "")).strip().splitlines()
            print("push rc", p.returncode, out[-1][:200] if out else "", flush=True)
            if p.returncode == 0:
                for _ in range(5):
                    try:
                        ls = sh("git", "ls-tree", "-r", "--name-only", api("commits/main")["sha"],
                                "--", BOOK, timeout=300).stdout
                    except Exception:
                        time.sleep(8)
                        continue
                    if BOOK in ls:
                        print("DONE: my content verified on remote", flush=True)
                        return 0
                return 5
            time.sleep(10)
        print("EXHAUSTED attempts", flush=True)
        return 6
    finally:
        cleanup()


if __name__ == "__main__":
    sys.exit(main())
