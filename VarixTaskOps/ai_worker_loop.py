# -*- coding: utf-8 -*-
"""
VE 产线 · AI 施工循环驱动器 v2（无人值守版）

与 v1 的区别：v1 的 --max 是假的（循环体里无条件 break，领一单就停）。
v2 把它改成真正的循环：领单 → 写交接 → 等施工 → 回写 → 领下一单，
直到队列空或达到 --max 上限。

★ 状态持久化兜底（2026-10-04 修的真缺陷）★
VTaskBoard.exe 是 PyInstaller onefile，运行时把 taskboard.md 解包到
%TEMP%\\_MEIxxxxx\\，所有 write_back 都写进那个临时目录——进程一退出，
所有"已完成"状态全部蒸发。本脚本因此自带台账 _ledger.jsonl：
每次 complete 之后，把该单的状态直接写回**仓库里的真 taskboard.md**，
不依赖 exe 的临时目录。重启 exe 后台账照样在。
（源码侧也已修 vtaskboard.py 的 MD_PATH 候选顺序：持久目录优先——
 exe 同级 → exe 上级 → 包内副本。）

★ 多 AI 防撞车（2026-10-04 加）★
多个 AI 会话可能同时对同一批任务施工。本脚本用两道锁挡住撞车：
1. 全局锁 _worker.lock —— 目录级排他锁，同一时刻只允许一个 AI 领单。
   抢不到锁就如实报错退出，不静默降级。
2. 施工锁 _active.lock —— 记录当前正在施工的任务 id 与持有者。
   领单时检查，若该单已被别的 worker 持有则跳过。
用法：--lock-status 看锁，--lock-release 强释（人工介入时用）。

★ 前置链纪律（2026-10-04 加）★
--claim 只领「前置已全部完成」的单。软件侧 is_unlocked 已保证，
本脚本额外读真 taskboard.md 的状态字段本地复核一遍（双保险——
 exe 的解锁判定可能因 md 副本过期而误判）。

用法：
  python ai_worker_loop.py --claim --worker AI-VE     # 领一单并写交接
  python ai_worker_loop.py --done VE-F0001 --result "..."   # 回写完成（幂等）
  python ai_worker_loop.py --block VE-F0001 --reason "..."  # 回写阻塞
  python ai_worker_loop.py --loop --max 20 --worker AI-VE   # 连续领 max 单（施工由外部 AI 做）
  python ai_worker_loop.py --status                      # 看台账与队列
  python ai_worker_loop.py --handoff                    # 生成跨对话交接 MD
  python ai_worker_loop.py --replay                     # 把台账重放进真 taskboard.md
  python ai_worker_loop.py --lock-status# 看锁状态
  python ai_worker_loop.py --lock-release               # 强释全局锁（人工）
  python ai_worker_loop.py --next                      # 只看下一单该是什么（不领）
"""
import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
BASE = "http://127.0.0.1:8767"

# 真 taskboard.md（仓库内，非 exe 临时目录）——状态回写的权威落点
REAL_MD = os.path.join(HERE, "VTaskBoard", "taskboard.md")
# 台账：exe 重启也不丢
LEDGER = os.path.join(HERE, "_ledger.jsonl")
# 当前施工任务交接件
ACTIVE = os.path.join(HERE, "_active_task.md")


# --------------------------------------------------------------------- #
# 多 AI 防撞车：全局锁 + 施工锁
# --------------------------------------------------------------------- #
LOCK_FILE = os.path.join(HERE, "_worker.lock")
ACTIVE_LOCK = os.path.join(HERE, "_active.lock")
LOCK_STALE_SEC = 3600  # 超过 1 小时视为陈旧锁（进程崩溃残留）


def lock_status():
    out = {"全局锁": "空闲", "施工锁": None}
    for path, key in ((LOCK_FILE, "全局锁"), (ACTIVE_LOCK, "施工锁")):
        if not os.path.exists(path):
            continue
        age = time.time() - os.path.getmtime(path)
        try:
            with open(path, encoding="utf-8") as f:
                info = json.load(f)
        except Exception:
            info = {"worker": "?"}
        info["年龄秒"] = int(age)
        info["陈旧"] = age > LOCK_STALE_SEC
        out[key] = info
    return out


def lock_acquire(worker):
    """抢全局锁。O_CREAT|O_EXCL 原子创建——抢不到就如实退出，不静默降级。"""
    if os.path.exists(LOCK_FILE):
        age = time.time() - os.path.getmtime(LOCK_FILE)
        if age <= LOCK_STALE_SEC:
            try:
                with open(LOCK_FILE, encoding="utf-8") as f:
                    holder = json.load(f).get("worker", "?")
            except Exception:
                holder = "?"
            return False, "全局锁被 %s 持有（%.0f 秒前），拒绝领单以免撞车" % (holder, age)
        # 陈旧锁：先删再抢
        try:
            os.remove(LOCK_FILE)
        except OSError:
            pass
    payload = json.dumps({"worker": worker, "pid": os.getpid(), "ts": int(time.time())},
                         ensure_ascii=False)
    try:
        fd = os.open(LOCK_FILE, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError:
        return False, "并发抢锁失败（另一 AI 刚抢到）"
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(payload)
    return True, "已持全局锁"


def lock_release():
    for pth in (LOCK_FILE, ACTIVE_LOCK):
        if os.path.exists(pth):
            try:
                os.remove(pth)
            except OSError:
                pass
    return True


def active_set(task_id, worker):
    payload = json.dumps({"task": task_id, "worker": worker, "ts": int(time.time())},
                         ensure_ascii=False)
    with open(ACTIVE_LOCK, "w", encoding="utf-8") as f:
        f.write(payload)


def active_clear():
    if os.path.exists(ACTIVE_LOCK):
        try:
            os.remove(ACTIVE_LOCK)
        except OSError:
            pass


def active_conflict(task_id):
    """该单是否已被别的 worker 持有施工。"""
    if not os.path.exists(ACTIVE_LOCK):
        return None
    try:
        with open(ACTIVE_LOCK, encoding="utf-8") as f:
            info = json.load(f)
    except Exception:
        return None
    if info.get("task") == task_id:
        return info.get("worker")
    return None


# --------------------------------------------------------------------- #
# 前置链本地复核（双保险）
# --------------------------------------------------------------------- #

def md_state_map():
    """读真 taskboard.md，取每单的状态/前置。本地复核用，不依赖 exe。"""
    states, pres = {}, {}
    if not os.path.exists(REAL_MD):
        return states, pres
    cur_id = None
    with open(REAL_MD, encoding="utf-8") as f:
        for ln in f:
            if ln.startswith("## ["):
                cur_id = None
                seg = ln[4:ln.find("]")] if "]" in ln else ""
                if seg:
                    cur_id = seg
                    states.setdefault(cur_id, "待领")
                    pres.setdefault(cur_id, "")
            elif cur_id:
                ls = ln.strip()
                if ls.startswith("- 状态:"):
                    states[cur_id] = ls.split(":", 1)[1].strip()
                elif ls.startswith("- 前置:"):
                    pres[cur_id] = ls.split(":", 1)[1].strip()
    return states, pres


def prereq_ok(task_id, states, pres):
    """前置是否已全部完成。空前置 = 无前置。未知前置 id 按未完成处理（保守）。"""
    p = pres.get(task_id, "")
    if not p:
        return True, "无前置"
    if states.get(p) == "已完成":
        return True, "前置 %s 已完成" % p
    return False, "前置 %s 状态为 %s" % (p, states.get(p, "未知"))


# --------------------------------------------------------------------- #
# HTTP
# --------------------------------------------------------------------- #
def api(path, body=None, timeout=30):
    if body is None:
        req = urllib.request.Request(BASE + path)
    else:
        req = urllib.request.Request(
            BASE + path,
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json"},
        )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        try:
            return json.loads(e.read().decode("utf-8"))
        except Exception:
            return {"ok": False, "err": f"HTTP {e.code}"}
    except Exception as e:
        return {"ok": False, "err": f"连不上任务板 {BASE}：{e}"}


# --------------------------------------------------------------------- #
# 台账（状态持久化的兜底）
# --------------------------------------------------------------------- #
def ledger_load():
    """读台账。损坏行跳过而不是整体崩——台账是证据，不能因一行坏就丢全部。"""
    out = {}
    if not os.path.exists(LEDGER):
        return out
    with open(LEDGER, encoding="utf-8") as f:
        for ln in f:
            ln = ln.strip()
            if not ln:
                continue
            try:
                r = json.loads(ln)
            except Exception:
                continue
            if r.get("id"):
                out[r["id"]] = r
    return out


def ledger_append(rec):
    with open(LEDGER, "a", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        f.flush()
        os.fsync(f.fileno())  # 断电也不丢


def md_apply(rec):
    """把一条状态记录写回真 taskboard.md（原子写）。找不到该单则如实报错，不静默。"""
    if not os.path.exists(REAL_MD):
        return False, f"真 taskboard.md 不存在：{REAL_MD}"
    with open(REAL_MD, encoding="utf-8") as f:
        lines = f.read().split("\n")
    # 定位块
    start = None
    for i, ln in enumerate(lines):
        if ln.startswith("## [") and f"[{rec['id']}]" in ln:
            start = i
            break
    if start is None:
        return False, f"taskboard.md 里找不到 {rec['id']}"
    end = len(lines)
    for i in range(start + 1, len(lines)):
        if lines[i].startswith("## ["):
            end = i
            break
    block = lines[start:end]
    newv = {
        "状态": rec.get("状态", "已完成"),
        "领取人": rec.get("领取人", ""),
        "结果": rec.get("结果", ""),
        "时间": rec.get("时间", ""),
    }
    for key, val in newv.items():
        hit = False
        for j, bl in enumerate(block):
            if bl.startswith(f"- {key}:"):
                block[j] = f"- {key}: {val}"
                hit = True
                break
        if not hit:
            block.append(f"- {key}: {val}")
    lines[start:end] = block
    tmp = REAL_MD + ".tmp"
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    os.replace(tmp, REAL_MD)
    return True, "已写入真 taskboard.md"


# --------------------------------------------------------------------- #
# 交接件
# --------------------------------------------------------------------- #
def write_active(t, worker):
    with open(ACTIVE, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"""# 当前任务（AI 施工中）

- ID: {t['id']}
- 标题: {t['title']}
- 状态: {t.get('状态')}（领取人 {t.get('领取人')}）
- 行数目标: {t.get('行数') or '见锚点'}
- 前置: {t.get('前置') or '无'}
- 书路径: {t.get('书路径') or '—'}
- 验收标准: {t.get('验收') or '按册内锚点原文'}

## 规格要点（施工前必读，来自册内锚点原文）

{t.get('简介', '（无）')}

## 施工完成后执行

```
python ai_worker_loop.py --done {t['id']} --result "<结果摘要>"
```

阻塞时执行：

```
python ai_worker_loop.py --block {t['id']} --reason "<原因>"
```
""")
        f.flush()
        os.fsync(f.fileno())


# --------------------------------------------------------------------- #
# 命令
# --------------------------------------------------------------------- #
def cmd_claim(worker):
    """领一单。三道闸：全局锁（防多 AI 撞车）→ 前置本地复核 → 软件侧 is_unlocked。"""
    got, msg = lock_acquire(worker)
    if not got:
        return {"ok": False, "err": msg, "锁": lock_status()}
    r = api("/api/claim", {"worker": worker})
    if not r.get("ok"):
        lock_release()
        return r
    t = r["task"]
    # 前置本地复核：软件侧已判解锁，这里再用真 md 兜一遍（双保险）
    states, pres = md_state_map()
    ok, why = prereq_ok(t["id"], states, pres)
    if not ok:
        # 复核不通过 ⇒ 退回软件，不硬闯（退回失败也如实报错，不静默）
        rb = api("/api/release", {"id": t["id"]})
        lock_release()
        return {"ok": False, "err": "前置未完成（本地复核）：%s" % why,
                "退回软件": rb.get("ok"), "task": t["id"]}
    # 施工锁：确认没有别的 worker 在做这一单
    holder = active_conflict(t["id"])
    if holder and holder != worker:
        api("/api/release", {"id": t["id"]})
        lock_release()
        return {"ok": False, "err": "该单正被 %s 施工，已退回以免撞车" % holder}
    write_active(t, worker)
    active_set(t["id"], worker)
    r["前置复核"] = why
    r["锁"] = lock_status()
    return r


def cmd_done(tid, worker, result):
    """回写完成。三路并写：exe API（界面即时可见）+ 台账（重启不丢）+ 真 md（仓库权威）。"""
    if not result:
        return {"ok": False, "err": "结果摘要为空，规格验收第③条要求非空，拒绝写完成"}
    ts = time.strftime("%m-%d %H:%M")
    rec = {"id": tid, "状态": "已完成", "领取人": worker,
           "结果": result[:200], "时间": ts, "ts": int(time.time())}
    out = {"ok": True, "id": tid}
    # 1) 台账（最可靠，先写）
    ledger_append(rec)
    out["ledger"] = "已记台账"
    # 2) exe API（界面即时可见；失败不致命，台账已兜底）
    r = api("/api/complete", {"id": tid, "worker": worker, "result": result})
    out["api"] = "ok" if r.get("ok") else f"未生效（{r.get('err')}）——台账已兜底，不影响最终一致"
    # 3) 真 taskboard.md（仓库权威，exe 重启也不丢）
    ok, msg = md_apply(rec)
    out["md"] = msg
    if not ok:
        out["ok"] = False
    # 交活完成 ⇒ 释放施工锁（全局锁留着，让同一 worker 继续领下一单）
    active_clear()
    # 4) 自动产出下一轮交接提示词（下一对话直接 @_next_prompt.md 即可）
    #
    # 为什么在这里做：交接内容必须反映「刚交完这单之后」的队列状态，
    # 由 --done 触发才能保证时机正确。独立跑 emit_next_prompt.py 也行，
    # 但那时状态可能已经变了。
    out["交接"] = emit_next_prompt()
    return out


def emit_next_prompt(task_id=None):
    """调用同目录的 emit_next_prompt.py 产出 _next_prompt.md。

    独立成文件是为了让「产出内容」与「领单/回写」两条逻辑线解耦，
    那边改判据、这边改循环，互不牵动。
    """
    import subprocess
    script = os.path.join(HERE, "emit_next_prompt.py")
    if not os.path.exists(script):
        return "未运行（emit_next_prompt.py 不存在）"
    cmd = [sys.executable, script]
    if task_id:
        cmd += ["--task", task_id]
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return "未产出（生成器超时 180s）"
    if p.returncode != 0:
        # 失败要能看出原因，但不打断主流程（交活本身已成功）
        detail = (p.stdout or p.stderr or "").strip().replace("\n", " ")
        return "未产出（生成器退出码 %d）：%s" % (p.returncode, detail[:200])
    return "已产出 _next_prompt.md（%s）" % (p.stdout or "").strip()[:80]


def cmd_block(tid, reason):
    ts = time.strftime("%m-%d %H:%M")
    rec = {"id": tid, "状态": "阻塞", "结果": reason[:200], "时间": ts, "ts": int(time.time())}
    ledger_append(rec)
    r = api("/api/block", {"id": tid, "reason": reason})
    md_apply(rec)
    return {"ok": True, "id": tid, "api": "ok" if r.get("ok") else r.get("err"), "md": "已写入"}


def cmd_status():
    led = ledger_load()
    done = [k for k, v in led.items() if v.get("状态") == "已完成"]
    blocked = [k for k, v in led.items() if v.get("状态") == "阻塞"]
    s = api("/api/summary")
    return {
        "台账已完成": len(done),
        "台账阻塞": len(blocked),
        "队列": s.get("by_status"),
        "分册": s.get("by_book"),
        "exe 当前 md": s.get("md"),
        "真实 md": REAL_MD,
    }


def cmd_handoff():
    """生成跨对话交接 MD：把所有 MD 上下文汇总，下一个对话读完即能接续。"""
    led = ledger_load()
    done = sorted([k for k, v in led.items() if v.get("状态") == "已完成"])
    blocked = sorted([k for k, v in led.items() if v.get("状态") == "阻塞"])
    s = api("/api/summary")
    md = os.path.join(HERE, "HANDOFF.md")
    with open(md, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"""# VE 产线交接单（跨对话续跑用）

> 生成时间：{time.strftime('%Y-%m-%d %H:%M:%S')}
> 本文件由 `ai_worker_loop.py --handoff` 自动生成，是**下一个对话的入口**。

## 一、怎么开工（下一个对话照此执行）

```bash
cd VarixTaskOps
python ai_worker_loop.py --claim --worker AI-VE   # 领一单，写 _active_task.md
# 读 _active_task.md 的"书路径"，去规格书读锚点原文，然后施工
python ai_worker_loop.py --done <ID> --result "<摘要>"   # 交活
```

循环到无单可领为止：`python ai_worker_loop.py --loop --max 50 --worker AI-VE`

## 二、当前进度

- 台账已完成：**{len(done)}** 单
- 台账阻塞：**{len(blocked)}** 单
- 队列剩余：{json.dumps(s.get('by_status'), ensure_ascii=False)}
- 分册：{json.dumps(s.get('by_book'), ensure_ascii=False)}

### 已完成清单
{chr(10).join('- ' + x for x in done) if done else '（暂无）'}

### 阻塞清单（需人工拍板）
{chr(10).join('- ' + x for x in blocked) if blocked else '（暂无）'}

## 三、必须知道的坑（踩过的，别再踩）

1. **exe 状态会丢**：`VTaskBoard.exe` 是 PyInstaller onefile，运行时把
   `taskboard.md` 解包到 `%TEMP%\\_MEIxxxxx\\`，写回只落临时目录，进程退出即蒸发。
   所以本脚本三路并写：exe API（界面）+ `_ledger.jsonl`（台账）+ 真 taskboard.md（仓库权威）。
   台账可用 `--replay` 重放进真 md。

2. **门禁工具不在 PATH**：本仓 `node_modules` 只有 playwright-core，没有 tsc/vitest；
   根 `package.json` 未入库。现成 tsc 在
   `C:/Users/varia/.vscode/extensions/ms-vscode.vscode-typescript-next-6.0.20260416/node_modules/typescript/lib/tsc.js`。
   跑法见 `_attic/2026-10-04-ve-产线/tests/run_tests.sh`。
   （npm 装 typescript 会失败：registry 证书过期 `CERT_HAS_EXPIRED`。）

3. **VE 册语言是 TypeScript**（`BOOK_LANG = {{"VE": "TypeScript"}}`），
   CoRun/CGPU 是 Rust。代码落 `src/features/<域>/`，VE 现阶段落 `src/features/ve/`。

4. **测试与脚本不入库**：纯功能代码才推 GitHub；测试/日志/截图全部归 `_attic/`。

## 四、关键路径

| 用途 | 路径 |
|---|---|
| 产线驱动器 | `VarixTaskOps/ai_worker_loop.py` |
| 状态台账 | `VarixTaskOps/_ledger.jsonl` |
| 当前任务交接 | `VarixTaskOps/_active_task.md` |
| 任务库（权威） | `VarixTaskOps/VTaskBoard/taskboard.md` |
| 软件 | `VarixTaskOps/VTaskBoard/dist/VTaskBoard.exe` |
| VE 规格书 | `docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md` |
| CGPU 规格书 | `docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md` |
| CoRun 规格书 | `docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md` |
| VE 源码 | `src/features/ve/` |
| 测试归档 | `_attic/2026-10-04-ve-产线/tests/` |
""")
    return {"ok": True, "handoff": md}


def cmd_next():
    """只报下一单该是什么（不领、不改状态）——让 AI 开工前先看清靶子。"""
    states, pres = md_state_map()
    api_sum = api("/api/summary")
    by_status = (api_sum.get("by_status") or {})
    # 从软件取第一张可领单
    r = api("/api/claim", {"worker": "__probe__"})
    if not r.get("ok"):
        return {"ok": False, "err": r.get("err"), "队列": by_status}
    t = r["task"]
    api("/api/release", {"id": t["id"]})   # 立即退回，保持队列不变
    ok, why = prereq_ok(t["id"], states, pres)
    return {
        "ok": True,
        "下一单": t["id"],
        "标题": t["title"],
        "册": t.get("册"),
        "域": t.get("域"),
        "行数目标": t.get("行数"),
        "书路径": t.get("书路径"),
        "前置复核": why,
        "队列": by_status,
    }


def cmd_replay():
    """把台账全部重放进真 taskboard.md（exe 重启丢状态后的抢救）。"""
    led = ledger_load()
    ok = fail = 0
    errs = []
    for tid, rec in led.items():
        good, msg = md_apply(rec)
        if good:
            ok += 1
        else:
            fail += 1
            errs.append(f"{tid}: {msg}")
    return {"ok": fail == 0, "已重放": ok, "失败": fail, "错误": errs[:10]}


def cmd_loop(worker, maxn):
    """连续领单循环。注意：施工由外部 AI 做，本函数只负责领单+写交接+记账。"""
    out = []
    for i in range(maxn):
        r = api("/api/claim", {"worker": worker})
        if not r.get("ok"):
            out.append({"第几单": i + 1, "结果": "无单可领", "原因": r.get("err")})
            print(f"[{i+1}/{maxn}] 无单可领：{r.get('err')}", flush=True)
            break
        t = r["task"]
        write_active(t, worker)
        print(f"[{i+1}/{maxn}] 领到 {t['id']} · {t['title']}（行数目标 {t.get('行数')}）", flush=True)
        out.append(t["id"])
    return out


def main():
    ap = argparse.ArgumentParser(description="VE 产线 AI 施工循环驱动器")
    ap.add_argument("--worker", default="AI-VE")
    ap.add_argument("--claim", action="store_true", help="领一单并写 _active_task.md")
    ap.add_argument("--loop", action="store_true", help="连续领单")
    ap.add_argument("--max", type=int, default=20)
    ap.add_argument("--done", metavar="ID")
    ap.add_argument("--result", default="")
    ap.add_argument("--block", metavar="ID")
    ap.add_argument("--reason", default="")
    ap.add_argument("--release", metavar="ID")
    ap.add_argument("--status", action="store_true")
    ap.add_argument("--handoff", action="store_true")
    ap.add_argument("--replay", action="store_true")
    ap.add_argument("--next", action="store_true", help="只报下一单是什么（不领）")
    ap.add_argument("--lock-status", action="store_true", help="看锁状态")
    ap.add_argument("--lock-release", action="store_true", help="强释全部锁（人工介入）")
    ap.add_argument("--emit-prompt", action="store_true",
                    help="生成下一轮交接提示词到 _next_prompt.md")
    ap.add_argument("--task", metavar="ID", help="配合 --emit-prompt 指定单号")
    ap.add_argument("--cancel", action="store_true")
    a = ap.parse_args()

    def p(x):
        print(json.dumps(x, ensure_ascii=False, indent=2))

    if a.cancel:
        p(api("/api/release_worker", {"worker": ""})); return
    if a.done:
        p(cmd_done(a.done, a.worker, a.result)); return
    if a.block:
        p(cmd_block(a.block, a.reason)); return
    if a.release:
        p(api("/api/release", {"id": a.release})); return
    if a.status:
        p(cmd_status()); return
    if a.handoff:
        p(cmd_handoff()); return
    if a.replay:
        p(cmd_replay()); return
    if a.next:
        p(cmd_next()); return
    if a.emit_prompt:
        p({"ok": True, "结果": emit_next_prompt(a.task)}); return
    if a.lock_status:
        p(lock_status()); return
    if a.lock_release:
        lock_release(); p({"ok": True, "已释放": ["全局锁", "施工锁"]}); return
    if a.claim:
        p(cmd_claim(a.worker)); return
    if a.loop:
        p({"领单": cmd_loop(a.worker, a.max)}); return

    ap.print_help()


if __name__ == "__main__":
    main()
