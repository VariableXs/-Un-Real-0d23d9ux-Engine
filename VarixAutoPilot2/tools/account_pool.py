"""TreeCode 账号池客户端 · 积分监控与账号切换。

2026-10-06 实测（Variable 指定：积分耗尽自动切号继续产线）：

TreeCode 控制台（D:/Treecode/TreeCode客户版/）本地 HTTP 服务：
  - GET  http://127.0.0.1:8792/api/quota?fresh=1   账号池总览
    platforms[0].accounts[] = {label, name, uin, total, used, remain,
                               ok, realm, checkin...}（label 是登录按钮的键）
    platforms[0].summary    = {accounts, remain, used, total, checkedIn, current}
  - POST http://127.0.0.1:8792/api/accounts/login  body {"label": "<label>"}
    → TreeCode 自动关掉 WorkBuddy 客户端再重开（十来秒），切到该账号。
    干跑实测：{"ok":false,"error":"账号池里没有这个号：xxx"}（label 校验在先，
    不会误切）。
  - POST http://127.0.0.1:8792/api/work/refresh-balance  强刷余额

关键实测事实：
  - summary.current.uin 与 accounts[].uin 关联不上（label=null, remain=0）
    ——「当前客户端账号还剩多少」TreeCode 侧不可靠。塔的判据改为：
    异常信号（连续发送失败 / 大量工人停止）→ 挑账号池里余量最高的号切过去；
    若最高余量都 < 门槛 → 判定「没有可用积分账号」→ 停机。
  - WorkBuddy 主进程命令行（2026-10-06 实测）：
    C:\\Users\\varia\\Desktop\\WorkBuddy\\WorkBuddy.exe
        --remote-debugging-port=9222 --remote-debugging-address=127.0.0.1
    TreeCode 重启客户端后 CDP 参数是否保留未知 → wait_workbuddy_back 里
    9222 迟迟不通时，塔接管：杀掉无参进程、用带参命令行重启（产线客户端，
    无用户数据风险；重启前 CDP 已断，无会话可丢）。
"""

import json
import subprocess
import time
import urllib.request

TC_BASE = "http://127.0.0.1:8792"
CDP_URL = "http://127.0.0.1:9222"

# WorkBuddy 带参启动命令（2026-10-06 实测主进程命令行）
WORKBUDDY_EXE = r"C:\Users\varia\Desktop\WorkBuddy\WorkBuddy.exe"
WORKBUDDY_ARGS = ["--remote-debugging-port=9222",
                  "--remote-debugging-address=127.0.0.1"]


def _get(path: str, timeout: float = 8.0):
    with urllib.request.urlopen(f"{TC_BASE}{path}", timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8", "replace"))


def _post(path: str, body: dict, timeout: float = 12.0):
    req = urllib.request.Request(
        f"{TC_BASE}{path}",
        data=json.dumps(body).encode("utf-8"),
        headers={"content-type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8", "replace"))


def quota(fresh: bool = False) -> dict:
    """账号池总览。fresh=True 强制拉新（服务端会现场刷各账号余额，稍慢）。"""
    return _get(f"/api/quota{'?fresh=1' if fresh else ''}")


def work_platform(q: dict | None = None) -> dict:
    p = (q or quota()).get("platforms", [])
    for x in p:
        if x.get("id") == "work":
            return x
    return p[0] if p else {}


def accounts(q: dict | None = None) -> list:
    """账号列表（label/uin/remain/ok/realm...）。"""
    return work_platform(q).get("accounts") or []


def summary(q: dict | None = None) -> dict:
    return work_platform(q).get("summary") or {}


def pick_richest(min_remain: float, exclude_labels: set | None = None) -> dict | None:
    """挑余量最高、ok、≥门槛 的账号。没有 → None（停机信号）。"""
    excl = exclude_labels or set()
    cand = [a for a in accounts()
            if a.get("ok") and (a.get("remain") or 0) >= min_remain
            and a.get("label") not in excl]
    if not cand:
        return None
    return max(cand, key=lambda a: a.get("remain") or 0)


def switch(label: str) -> tuple:
    """触发切号（TreeCode 会自动重启 WorkBuddy 客户端）。返回 (ok, message)。"""
    try:
        r = _post("/api/accounts/login", {"label": label})
    except Exception as e:
        return False, f"login API 异常：{e}"
    if r.get("ok"):
        return True, r.get("message") or "已切换"
    return False, r.get("error") or r.get("message") or str(r)


def cdp_alive(timeout: float = 2.5) -> bool:
    try:
        with urllib.request.urlopen(f"{CDP_URL}/json/version", timeout=timeout) as r:
            return r.status == 200
    except Exception:
        return False


def _workbuddy_pids() -> list:
    """WorkBuddy.exe 的全部 PID（taskkill 用）。"""
    try:
        out = subprocess.run(
            ["taskkill", "/FI", "IMAGENAME eq WorkBuddy.exe", "/FO", "CSV", "/N"],
            capture_output=True, text=True, timeout=15)
        # 无进程时 taskkill 报错且无 CSV 行；这里只用于「杀前确认存在」
        return [] if "信息: 没有" in (out.stdout + out.stderr) else ["?"]
    except Exception:
        return ["?"]


def _kill_workbuddy() -> None:
    subprocess.run(["taskkill", "/F", "/IM", "WorkBuddy.exe", "/T"],
                   capture_output=True, text=True, timeout=25)
    time.sleep(2.5)


def _launch_workbuddy() -> None:
    """用带 CDP 参数的命令行重启 WorkBuddy（产线主进程参数，2026-10-06 实测）。"""
    subprocess.Popen([WORKBUDDY_EXE] + WORKBUDDY_ARGS,
                     cwd=str(WORKBUDDY_EXE.rsplit("\\", 1)[0]),
                     creationflags=subprocess.CREATE_NEW_PROCESS_GROUP
                     | subprocess.DETACHED_PROCESS)


def wait_workbuddy_back(timeout_s: float = 300.0,
                        log=print) -> tuple:
    """等 WorkBuddy（CDP 9222）回来；迟迟不回则塔接管重启。

    流程：
      1. 轮询 9222 至 90s（TreeCode 重启通常十来秒）；
      2. 仍不通 → 杀掉无参 WorkBuddy（若有）→ 带参重启 → 再轮询至 timeout；
      3. 超时 → (False, 原因)。
    返回 (ok, evidence)。
    """
    ev = {"phase": "wait", "took_s": 0.0}
    t0 = time.time()
    while time.time() - t0 < 90:
        if cdp_alive():
            ev["took_s"] = round(time.time() - t0, 1)
            ev["phase"] = "treecode_reboot_ok"
            return True, ev
        time.sleep(3)
    # 塔接管：带参重启
    log("[切号] 90s 内 9222 未恢复 → 塔接管重启 WorkBuddy（带 CDP 参数）")
    ev["phase"] = "tower_reboot"
    try:
        _kill_workbuddy()
        ev["killed"] = True
    except Exception as e:
        ev["kill_err"] = str(e)
    time.sleep(2)
    try:
        _launch_workbuddy()
        ev["launched"] = True
    except Exception as e:
        return False, {**ev, "launch_err": str(e)}
    t1 = time.time()
    while time.time() - t1 < timeout_s:
        if cdp_alive():
            ev["took_s"] = round(time.time() - t0, 1)
            return True, ev
        time.sleep(3)
    return False, {**ev, "why": f"重启后 {timeout_s:.0f}s 内 9222 仍未恢复"}


def probe_all(min_remain: float) -> dict:
    """只读体检：池子规模/总余量/最高余量/可选号数（供 --probe 与心跳日志）。"""
    try:
        q = quota(fresh=False)
        accs = accounts(q)
        s = summary(q)
        ok_accs = [a for a in accs if a.get("ok")]
        rich = max(ok_accs, key=lambda a: a.get("remain") or 0) if ok_accs else None
        return {
            "ok": True,
            "accounts": len(accs),
            "pool_remain": round(s.get("remain") or 0, 1),
            "richest": (rich or {}).get("label"),
            "richest_remain": round((rich or {}).get("remain") or 0, 1),
            "switchable": sum(1 for a in ok_accs
                              if (a.get("remain") or 0) >= min_remain),
        }
    except Exception as e:
        return {"ok": False, "why": str(e)}
