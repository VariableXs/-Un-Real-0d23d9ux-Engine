# -*- coding: utf-8 -*-
"""AI-2 · S1.1 盘体性能实测（严格只读）。

对应《实施总步骤图》S1.1 / 分工图 AI-2 S1.1：
  顺序 >=400MB/s 且 4K >=20MB/s 才过门（进 S1.2 的前置）。

纪律：
  - 全程零写入（安全闸：纯只读）。只用盘上既有文件做读路径测速。
  - CreateFileW + FILE_FLAG_NO_BUFFERING 绕过 Windows 文件缓存，测真实设备吞吐；
    读写缓冲区用 VirtualAlloc 页对齐（NO_BUFFERING 硬要求）。
  - X: WIN_ENGINE 为空（无既有数据可读），物理同盘同链路，用 S:/W:/V: 既有文件代表；
    结论里如实注明。

用法：
  python _attic/ai2-s111-perf.py            # 全量跑（盘点+顺序+4K）
  python _attic/ai2-s111-perf.py --inventory # 只盘点候选文件
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import random
import sys
import time

GENERIC_READ = 0x80000000
FILE_SHARE_READ = 0x1
FILE_SHARE_WRITE = 0x2
OPEN_EXISTING = 3
FILE_FLAG_NO_BUFFERING = 0x20000000
FILE_FLAG_SEQUENTIAL_SCAN = 0x08000000
FILE_FLAG_RANDOM_ACCESS = 0x10000000
INVALID_HANDLE_VALUE = wt.HANDLE(-1).value
MEM_COMMIT = 0x1000
MEM_RESERVE = 0x2000
PAGE_READWRITE = 0x4

k32 = ctypes.WinDLL("kernel32", use_last_error=True)

VOLUMES = ["S:\\", "W:\\", "V:\\"]
SEQ_ROUND_BUDGET = 1 * 1024 * 1024 * 1024  # 每轮顺序读预算 1GiB
SEQ_CHUNK = 1 * 1024 * 1024                # 顺序读块 1MiB（对齐 4K）
SMALL_CHUNK = 4096
RAND_OPS = 8000                          # 4K 随机读操作数
MAX_WALK_ENTRIES = 60000                 # 每卷目录遍历上限（防大树拖死）
MAX_WALK_SECONDS = 90

GATE_SEQ_MBS = 400.0
GATE_RAND4K_MBS = 20.0


class FileErr(Exception):
    pass


def alloc_buf(size):
    k32.VirtualAlloc.restype = ctypes.c_void_p
    k32.VirtualAlloc.argtypes = [wt.LPVOID, ctypes.c_size_t, wt.DWORD, wt.DWORD]
    p = k32.VirtualAlloc(None, ctypes.c_size_t(size), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
    if not p:
        raise FileErr(f"VirtualAlloc failed: {ctypes.get_last_error()}")
    return p


def free_buf(p):
    k32.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, wt.DWORD]
    k32.VirtualFree.restype = wt.BOOL
    k32.VirtualFree(ctypes.c_void_p(p), ctypes.c_size_t(0), 0x8000)  # MEM_RELEASE


def open_ro(path, flags):
    k32.CreateFileW.restype = wt.HANDLE
    k32.CreateFileW.argtypes = [wt.LPCWSTR, wt.DWORD, wt.DWORD, wt.LPVOID, wt.DWORD, wt.DWORD, wt.HANDLE]
    h = k32.CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None, OPEN_EXISTING, flags, None)
    if h == INVALID_HANDLE_VALUE or not h:
        raise FileErr(f"CreateFileW failed({ctypes.get_last_error()}): {path}")
    return h


def read_at(h, buf_ptr, size, offset):
    """NO_BUFFERING 读：offset 与 size 都须落在扇区边界上。返回实际读取字节。"""
    k32.ReadFile.restype = wt.BOOL
    k32.ReadFile.argtypes = [wt.HANDLE, ctypes.c_void_p, wt.DWORD,
                             ctypes.POINTER(wt.DWORD), wt.LPVOID]
    k32.SetFilePointerEx.restype = wt.BOOL
    k32.SetFilePointerEx.argtypes = [wt.HANDLE, ctypes.c_longlong, ctypes.POINTER(ctypes.c_longlong), wt.DWORD]
    li = wt.LARGE_INTEGER(offset)
    if not k32.SetFilePointerEx(h, li, None, 0):
        raise FileErr(f"SetFilePointerEx failed: {ctypes.get_last_error()}")
    got = wt.DWORD(0)
    overlapped = None
    if not k32.ReadFile(h, buf_ptr, wt.DWORD(size), ctypes.byref(got), overlapped):
        raise FileErr(f"ReadFile failed: {ctypes.get_last_error()}")
    return got.value


def close_h(h):
    k32.CloseHandle(ctypes.c_void_p(h))


def walk_volume(root):
    """返回 [(path, size)]，按 size 降序；带条目数与时长上限。"""
    out = []
    t0 = time.perf_counter()
    count = 0
    for dirpath, dirnames, filenames in os.walk(root):
        if count > MAX_WALK_ENTRIES or (time.perf_counter() - t0) > MAX_WALK_SECONDS:
            break
        # 跳过系统目录影子（回收站/系统卷信息）
        dirnames[:] = [d for d in dirnames if not d.startswith("$")]
        for fn in filenames:
            if fn.startswith("$"):
                continue
            p = os.path.join(dirpath, fn)
            try:
                st = os.stat(p, follow_symlinks=False)
                if st.st_size >= MIN_FILE_MIB * 1024 * 1024:
                    out.append((p, st.st_size))
                    count += 1
            except OSError:
                continue
    out.sort(key=lambda x: -x[1])
    return out


MIN_FILE_MIB = 16


def seq_test(files, budget_bytes):
    """顺序读：文件循环直读（NO_BUFFERING 绕缓存，重复读仍是真实设备路径），
    1MiB 块，直至预算或无文件。返回 (mbs, bytes_read, passes, dt)。"""
    if not files:
        return (0.0, 0, 0, 0.0)
    buf = alloc_buf(SEQ_CHUNK)
    total = 0
    passes = 0
    t0 = time.perf_counter()
    try:
        while total < budget_bytes:
            progressed = False
            for path, size in files:
                if total >= budget_bytes:
                    break
                h = open_ro("\\\\?\\" + os.path.abspath(path),
                            FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN)
                try:
                    want = min(size, budget_bytes - total)
                    want = (want // SEQ_CHUNK) * SEQ_CHUNK
                    off = 0
                    while off < want:
                        n = read_at(h, buf, SEQ_CHUNK, off)
                        if n == 0:
                            break
                        off += n
                        total += n
                        progressed = True
                finally:
                    close_h(h)
            if not progressed:
                break
            passes += 1
        dt = time.perf_counter() - t0
        return (total / dt / (1024 * 1024) if dt > 0 else 0.0, total, passes, dt)
    finally:
        free_buf(buf)


def rand4k_test(path, size, ops):
    """4K 随机读：在单个大文件内随机 4K 对齐偏移。返回 (mbs, iops)。"""
    buf = alloc_buf(SMALL_CHUNK)
    rng = random.Random(20260921)
    max_off = ((size - SMALL_CHUNK) // SMALL_CHUNK) * SMALL_CHUNK
    if max_off <= 0:
        free_buf(buf)
        return (0.0, 0.0)
    h = open_ro("\\\\?\\" + os.path.abspath(path), FILE_FLAG_NO_BUFFERING | FILE_FLAG_RANDOM_ACCESS)
    t0 = time.perf_counter()
    total = 0
    try:
        for _ in range(ops):
            off = rng.randrange(0, max_off + 1, SMALL_CHUNK)
            n = read_at(h, buf, SMALL_CHUNK, off)
            total += n
        dt = time.perf_counter() - t0
        return (total / dt / (1024 * 1024), ops / dt)
    finally:
        close_h(h)
        free_buf(buf)


def main():
    inventory_only = "--inventory" in sys.argv
    print("=== S1.1 盘体只读实测（NO_BUFFERING，零写入） ===")
    all_files = {}
    for vol in VOLUMES:
        if not os.path.exists(vol):
            print(f"[skip] {vol} 不在场")
            continue
        fs_ = walk_volume(vol)
        all_files[vol] = fs_
        gb = sum(s for _, s in fs_) / (1024 ** 3)
        print(f"[inventory] {vol} 候选大文件(>=16MiB): {len(fs_)} 个 / {gb:.1f} GiB")
    if inventory_only:
        for vol, fs_ in all_files.items():
            for p, s in fs_[:5]:
                print(f"    {s/(1024**2):8.0f} MiB  {p}")
        return

    report = {"gate": {"seqMBs": GATE_SEQ_MBS, "rand4kMBs": GATE_RAND4K_MBS}, "volumes": {}}
    for vol, fs_ in all_files.items():
        if not fs_:
            report["volumes"][vol] = {"error": "无 >=16MiB 既有文件可测"}
            continue
        big = max(fs_, key=lambda x: x[1])
        # 顺序：3 轮独立测量（每轮 1GiB 预算），判定取全部轮 >= 门
        rounds = []
        for r in range(3):
            mbs, total, passes, dt = seq_test(fs_, SEQ_ROUND_BUDGET)
            rounds.append({"round": r + 1, "seqMBs": round(mbs, 1), "seqBytes": total,
                           "seqPasses": passes, "seqSec": round(dt, 2)})
            print(f"[test] {vol} 顺序 轮{r+1}: {mbs:7.1f} MB/s "
                  f"({total/(1024**2):.0f} MiB / {passes} 遍 / {dt:.1f}s)")
        seq_mbs_min = min(x["seqMBs"] for x in rounds)
        seq_mbs_avg = sum(x["seqMBs"] for x in rounds) / len(rounds)
        seq_pass = seq_mbs_min >= GATE_SEQ_MBS
        # 4K：两轮独立，判定取后轮（首轮含设备热身，如实记录）
        r1 = rand4k_test(big[0], big[1], RAND_OPS)
        r_mbs, iops = rand4k_test(big[0], big[1], RAND_OPS)
        r4k_pass = r_mbs >= GATE_RAND4K_MBS
        report["volumes"][vol] = {
            "seqRounds": rounds, "seqMBsMin": round(seq_mbs_min, 1), "seqMBsAvg": round(seq_mbs_avg, 1),
            "seqPass": seq_pass,
            "rand4kMBsRound1": round(r1[0], 2), "rand4kMBs": round(r_mbs, 1), "rand4kIOPS": round(iops, 0),
            "rand4kFile": os.path.basename(big[0]),
            "rand4kPass": r4k_pass,
        }
        print(f"[test] {vol} 顺序判定: 最小 {seq_mbs_min:.1f} / 均值 {seq_mbs_avg:.1f} MB/s "
              f"{'PASS' if seq_pass else 'FAIL'}")
        print(f"[test] {vol} 4K随机: 首轮 {r1[0]:.2f} → 复测 {r_mbs:.2f} MB/s ({iops:.0f} IOPS) "
              f"{'PASS' if r4k_pass else 'FAIL'}   [file={os.path.basename(big[0])}]")

    ok_vols = [v for v, r in report["volumes"].items() if r.get("seqPass") and r.get("rand4kPass")]
    tested = [v for v, r in report["volumes"].items() if "seqRounds" in r]
    verdict = "PASS" if tested and len(ok_vols) == len(tested) else "FAIL"
    report["verdict"] = verdict
    print(f"=== 总判定: {verdict}（过门卷 {len(ok_vols)}/{len(tested)}；门 顺序>={GATE_SEQ_MBS:.0f}MB/s 4K>={GATE_RAND4K_MBS:.0f}MB/s） ===")
    out = os.path.join(os.path.dirname(__file__), "ai2-s111-perf-result.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)
    print(f"结果已写 {out}")


if __name__ == "__main__":
    main()
