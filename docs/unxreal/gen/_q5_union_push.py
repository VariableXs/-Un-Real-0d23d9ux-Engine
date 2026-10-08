# -*- coding: utf-8 -*-
"""AI-105 union 推送器（判例承 AI-109：union 树恰为本会话产物）。

远端 base = api.github.com 实时 ref；union 内容恰为 9 件：
  1 独立增补册 + 6 生成器/数据卷（本地工作区字节）
  2 分工完成图 = 远端现态 + AI-105 进度登记块（幂等：已含则不重附）
  3 统一协作总台账 = 远端现态 + AI-105 会话条目 + R-PROC-002 补记（幂等同上）
主汇编册不入 union（253MB→104MB 态由持锁会话镜像日合并清偿，沿先例欠账登记）。
用法：python docs/unxreal/gen/_q5_union_push.py
"""
import base64
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, os.path.join(ROOT, "_attic", "tools"))
os.chdir(ROOT)

import gh_api_push as G  # noqa: E402  （复用 token 与 api() 通道）

BOOK = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md"
GENS = ["docs/unxreal/gen/_q5_firstprod.py",
        "docs/unxreal/gen/_q5_data1.py",
        "docs/unxreal/gen/_q5_data2.py",
        "docs/unxreal/gen/_q5_data3.py",
        "docs/unxreal/gen/_q5_data4.py",
        "docs/unxreal/gen/_q5_data5.py"]
DIAGRAM = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · AI分工完成图.md"
LEDGER = "CoRun Varix STAR II · Unxreal · 统一协作总台账.md"
MSG_PATH = os.path.join(ROOT, "_attic", "reports", "_q5_commit_msg.txt")
LOCAL_COMMIT = "a872a083"

DIAGRAM_MARK = "AI-105 · W90-Q5 首产段进度登记"
LEDGER_MARKS = ["AI-105 · W90-Q5 UWP/MSIX 现代应用模型 首产段 300 项立账",
                "AI-105 会话补记 · R-PROC-002 现场复发观察"]


def local_block(path, start_mark):
    """从本地共享文件截取「本会话块」（从 start_mark 起到文件尾）。
    前提：本会话块是该文件最后一段追加（与落件顺序一致）。"""
    with open(path, "rb") as f:
        data = f.read()
    mark = start_mark.encode("utf-8")
    pos = data.find(mark)
    assert pos >= 0, f"local block mark not found in {path}"
    # 向前含分隔线（若紧邻 ---）
    pre = data.rfind(b"\n---\n", 0, pos)
    if pre >= 0 and pos - pre < 16:
        pos = pre + 1
    return data[pos:]


def git_ident():
    out = subprocess.run(["git", "log", "-1", "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI",
                          LOCAL_COMMIT], capture_output=True, text=True, encoding="utf-8").stdout
    ident = [x.strip() for x in out.split("\0")]
    return {"name": ident[0], "email": ident[1], "date": ident[2]}, \
           {"name": ident[3], "email": ident[4], "date": ident[5]}


def remote_file_bytes(ref_commit, path):
    """经 git Data API 取远端现态文件字节（blob 通道，>1MB 亦稳）。"""
    commit = G.api("GET", f"git/commits/{ref_commit}")
    tree = G.api("GET", f"git/trees/{commit['tree']['sha']}?recursive=1")
    for item in tree.get("tree", []):
        if item["path"] == path and item["type"] == "blob":
            blob = G.api("GET", f"git/blobs/{item['sha']}")
            return base64.b64decode(blob["content"])
    return None  # 远端尚无此文件


def push_once():
    ref = G.api("GET", "git/ref/heads/main")["object"]["sha"]
    print(f"remote main = {ref[:10]}")
    items = []

    # 本会话 7 件：本地工作区字节
    for p in [BOOK] + GENS:
        with open(p, "rb") as f:
            content = f.read()
        blob = G.api("POST", "git/blobs", {"content": base64.b64encode(content).decode(),
                                           "encoding": "base64"})
        items.append({"path": p, "mode": "100644", "type": "blob", "sha": blob["sha"]})
        print(f"  blob + {p} ({len(content)} B)")

    # 分工完成图 union
    rem_diag = remote_file_bytes(ref, DIAGRAM)
    if rem_diag is None:
        raise SystemExit("远端无分工完成图——状态异常，拒绝盲建")
    if DIAGRAM_MARK.encode() in rem_diag:
        print("  diagram: AI-105 块已在远端（幂等跳过）")
        union = None
    else:
        union = rem_diag + b"\n---\n\n" + local_block(DIAGRAM, DIAGRAM_MARK)
        blob = G.api("POST", "git/blobs", {"content": base64.b64encode(union).decode(),
                                           "encoding": "base64"})
        items.append({"path": DIAGRAM, "mode": "100644", "type": "blob", "sha": blob["sha"]})
        print(f"  blob ~ {DIAGRAM} ({len(rem_diag)} -> {len(union)} B)")

    # 台账 union
    rem_led = remote_file_bytes(ref, LEDGER)
    if rem_led is None:
        raise SystemExit("远端无协作总台账——状态异常，拒绝盲建")
    marks_hit = [m for m in LEDGER_MARKS if m.encode() in rem_led]
    if len(marks_hit) == len(LEDGER_MARKS):
        print("  ledger: AI-105 条目与补记已在远端（幂等跳过）")
    else:
        add = b""
        if LEDGER_MARKS[0].encode() not in rem_led:
            add += b"\n---\n\n" + local_block(LEDGER, LEDGER_MARKS[0]).split(b"\n### " + LEDGER_MARKS[1].encode())[0].rstrip() + b"\n"
        if LEDGER_MARKS[1].encode() not in rem_led:
            add += b"\n" + local_block(LEDGER, "### " + LEDGER_MARKS[1])
        union = rem_led.rstrip() + b"\n" + add.lstrip(b"\n") + b"\n"
        blob = G.api("POST", "git/blobs", {"content": base64.b64encode(union).decode(),
                                           "encoding": "base64"})
        items.append({"path": LEDGER, "mode": "100644", "type": "blob", "sha": blob["sha"]})
        print(f"  blob ~ {LEDGER} ({len(rem_led)} -> {len(union)} B)")

    tree = G.api("POST", "git/trees", {"base_tree": ref, "tree": items})
    with open(MSG_PATH, "r", encoding="utf-8") as f:
        msg = f.read().rstrip("\n")
    msg += ("\n\n（本提交为 api.github.com union 载体：恰为本会话产物 9 件——独立增补册 1 + 生成器与数据卷 6 + "
            "AI分工完成图 AI-105 进度登记块 + 统一协作总台账 AI-105 会话条目与 R-PROC-002 复发观察补记；"
            "本地兄弟提交 " + LOCAL_COMMIT + "；主汇编册沿 AI-68/71/72/76/78/81/82/86/90/91/96/99/109 先例"
            "不入 union 待镜像日合并清偿；他会话在途产物零触碰）")
    author, committer = git_ident()
    new = G.api("POST", "git/commits", {"message": msg, "tree": tree["sha"],
                                        "parents": [ref], "author": author, "committer": committer})
    G.api("PATCH", "git/refs/heads/main", {"sha": new["sha"], "force": False})
    return new["sha"]


for attempt in range(1, 4):
    try:
        sha = push_once()
        print(f"PUSHED(union) {sha[:10]}  files=9  attempt={attempt}")
        sys.exit(0)
    except SystemExit:
        raise
    except Exception as e:  # 竞态：ref 被并行推送移动 → 重读重建
        print(f"attempt {attempt} failed: {e}")
        time.sleep(3)
raise SystemExit("union push 三次重试耗尽——登记欠账，稍后重试")
