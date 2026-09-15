#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""M0 验收监测：连续观测 Variable 宿主是否发生重启。

观测口径（三条独立证据，互相印证）：
  1. variable.exe 进程 pid 集合变化（出现/消失）
  2. 每个 pid 的进程创建时间（重启必变 -> 新的 CreationDate）
  3. %APPDATA%/com.variable.app/logs/variable.log 中 "app bootstrap" 行数增长

任一条发生变化即计一次 "restart candidate"。
默认观测 600 秒（M0 验收标准窗口），每 10 秒采样一次。

用法：
  python watch_host_restart.py [seconds] [interval]
"""
import ctypes
import ctypes.wintypes as wt
import datetime as dt
import json
import os
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000


class FILETIME(ctypes.Structure):
    _fields_ = [("dwLowDateTime", wt.DWORD), ("dwHighDateTime", wt.DWORD)]


def ft_to_dt(ft: FILETIME):
    v = (ft.dwHighDateTime << 32) | ft.dwLowDateTime
    if v == 0:
        return None
    epoch = v / 10_000_000 - 11_644_473_600
    try:
        return dt.datetime.fromtimestamp(epoch)
    except OSError:
        return None


def pid_creation(pid: int):
    h = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return None
    try:
        ct, et, kt, ut = FILETIME(), FILETIME(), FILETIME(), FILETIME()
        if k32.GetProcessTimes(h, ctypes.byref(ct), ctypes.byref(et),
                               ctypes.byref(kt), ctypes.byref(ut)):
            return ft_to_dt(ct)
        return None
    finally:
        k32.CloseHandle(h)


EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)


def snapshot_pids():
    snap = k32.CreateToolhelp32Snapshot(0x00000002, 0)  # TH32CS_SNAPPROCESS
    if snap in (0, -1, 0xFFFFFFFFFFFFFFFF):
        return {}
    res = {}

    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = [
            ("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD),
            ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", wt.DWORD), ("cntThreads", wt.DWORD),
            ("th32ParentProcessID", wt.DWORD), ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", wt.DWORD), ("szExeFile", ctypes.c_wchar * 260),
        ]

    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
    ok = k32.Process32FirstW(snap, ctypes.byref(entry))
    while ok:
        name = entry.szExeFile
        if name.lower() in ("variable.exe", "varia.exe"):
            res[entry.th32ProcessID] = name
        ok = k32.Process32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return res


def windows_of_pids(pids):
    """统计每个 pid 的可见顶层窗口数（同一 pip 的所有顶层窗口，含无标题）。"""
    per = {p: [] for p in pids}
    pid_box = {}

    def cb(hwnd, _l):
        pid = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        p = pid.value
        if p in per:
            length = u32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(length + 1)
            u32.GetWindowTextW(hwnd, buf, length + 1)
            cls = ctypes.create_unicode_buffer(256)
            u32.GetClassNameW(hwnd, cls, 256)
            vis = bool(u32.IsWindowVisible(hwnd))
            hw = hwnd.value if hasattr(hwnd, "value") else int(hwnd)
            if vis:
                pid_box.setdefault(p, []).append(
                    {"hwnd": hw, "class": cls.value[:60], "title": buf.value[:80]}
                )
        return True

    u32.EnumWindows(EnumWindowsProc(cb), 0)
    return pid_box


def log_path():
    base = os.environ.get("APPDATA") or os.path.expanduser("~")
    return os.path.join(base, "com.variable.app", "logs", "variable.log")


def bootstrap_count():
    p = log_path()
    try:
        with open(p, "r", encoding="utf-8", errors="replace") as f:
            return sum(1 for ln in f if "app bootstrap" in ln)
    except FileNotFoundError:
        return -1


def main():
    total_s = int(sys.argv[1]) if len(sys.argv) > 1 else 600
    interval = int(sys.argv[2]) if len(sys.argv) > 2 else 10

    out = {
        "started_at": dt.datetime.now().isoformat(timespec="seconds"),
        "window_seconds": total_s,
        "interval_seconds": interval,
        "samples": [],
        "events": [],
        "restart_candidates": 0,
    }

    prev_pids = set()
    prev_boot = bootstrap_count()
    prev_creat = {}
    prev_wins = {}
    t_end = time.time() + total_s
    n = 0

    print(f"[watch] variable.log = {log_path()}")
    print(f"[watch] baseline: pids={sorted(prev_pids)} bootstrap_lines={prev_boot}")

    while time.time() < t_end:
        n += 1
        now = dt.datetime.now()
        pids = set(snapshot_pids().keys())
        boot = bootstrap_count()
        creat = {p: pid_creation(p) for p in pids}
        wins = windows_of_pids(pids)
        ts = now.isoformat(timespec="seconds")

        events = []
        for p in sorted(pids - prev_pids):
            events.append(f"NEW variable.exe pid={p} created={creat.get(p)}")
        for p in sorted(prev_pids - pids):
            events.append(f"GONE variable.exe pid={p}")
        for p in sorted(pids & prev_pids):
            if prev_creat.get(p) != creat.get(p):
                events.append(f"RECREATED-INPLACE pid={p} {prev_creat.get(p)} -> {creat.get(p)}")
        if prev_boot >= 0 and boot > prev_boot:
            events.append(f"BOOTSTRAP +{boot - prev_boot} (total {boot})")
        for p in sorted(pids):
            a = len(prev_wins.get(p, []))
            b = len(wins.get(p, []))
            if prev_pids and p in prev_pids and a != b:
                events.append(f"WINDOWS pid={p}: {a} -> {b}")

        sample = {
            "t": ts,
            "pids": sorted(pids),
            "created": {str(p): (c.isoformat(timespec="seconds") if c else None)
                        for p, c in creat.items()},
            "bootstrap_lines": boot,
            "visible_windows": {str(p): len(v) for p, v in wins.items()},
        }
        out["samples"].append(sample)

        if events:
            for e in events:
                print(f"[{ts}] EVENT {e}")
            out["events"].append({"t": ts, "items": events})
            out["restart_candidates"] += sum(
                1 for e in events if e.startswith(("NEW", "GONE", "RECREATED", "BOOTSTRAP"))
            )

        prev_pids, prev_boot, prev_creat, prev_wins = pids, boot, creat, wins
        time.sleep(interval)

    out["finished_at"] = dt.datetime.now().isoformat(timespec="seconds")
    out["samples_taken"] = n
    here = os.path.dirname(os.path.abspath(__file__))
    rp = os.path.join(here, "..", "qa", "m0-restart-watch.json")
    rp = os.path.abspath(rp)
    os.makedirs(os.path.dirname(rp), exist_ok=True)
    with open(rp, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=2)
    print(f"[watch] done samples={n} restart_candidates={out['restart_candidates']}")
    print(f"[watch] report -> {rp}")


if __name__ == "__main__":
    main()
